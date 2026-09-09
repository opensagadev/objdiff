//! Conservative recovery for linked ELF32 i386. No instruction relocations are synthesized.
use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    string::{String, ToString},
    vec,
    vec::Vec,
};

use iced_x86::{
    Code, FlowControl, Instruction, InstructionInfoFactory, OpAccess, OpKind, Register,
};
use object::{Object as _, ObjectSection as _, ObjectSymbol as _, ObjectSymbolTable as _, elf};

use super::ArchX86;
use crate::obj::{RecoveredReference, RecoveredTarget};

#[derive(Debug, Default)]
pub(super) struct Linked {
    // Function address is part of the key: an overlapping symbol cannot lend facts to another.
    pub references: BTreeMap<(u64, u64), RecoveredReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Target {
    name: String,
    section: Option<String>,
    address: u64,
    size: u64,
}

fn target(file: &object::File, sym: &object::Symbol) -> Option<Target> {
    let name = sym.name().ok()?;
    if name.is_empty()
        || !matches!(
            sym.kind(),
            object::SymbolKind::Data | object::SymbolKind::Text | object::SymbolKind::Unknown
        )
    {
        return None;
    }
    let section = match sym.section_index() {
        Some(index) => {
            let section = file.section_by_index(index).ok()?;
            let offset = sym.address().checked_sub(section.address())?;
            if offset >= section.size() || sym.size() > section.size() - offset {
                return None;
            }
            Some(section.name().ok()?.to_string())
        }
        None if sym.is_undefined() => None,
        _ => return None,
    };
    Some(Target { name: name.into(), section, address: sym.address(), size: sym.size() })
}

/// Build disjoint intervals once. Every distinct alias/overlap is ambiguous, including
/// exact starts inside another symbol. Identical symtab/dynsym records are deduplicated.
fn intervals(targets: &[Target]) -> BTreeMap<u64, Option<usize>> {
    let mut events = BTreeMap::<u64, Vec<(usize, bool)>>::new();
    for (i, t) in targets.iter().enumerate() {
        let Some(end) = t.address.checked_add(t.size.max(1)) else { continue };
        events.entry(t.address).or_default().push((i, true));
        events.entry(end).or_default().push((i, false));
    }
    let mut active = BTreeSet::new();
    let mut out = BTreeMap::new();
    for (addr, events) in events {
        for (i, start) in events {
            if start {
                active.insert(i);
            } else {
                active.remove(&i);
            }
        }
        out.insert(addr, if active.len() == 1 { active.first().copied() } else { None });
    }
    out
}

/// Shared address resolver for relocated slot contents and linked GOTOFF operands.
struct SymbolIndex {
    targets: Vec<Target>,
    intervals: BTreeMap<u64, Option<usize>>,
}

impl SymbolIndex {
    fn new(targets: Vec<Target>) -> Self { Self { intervals: intervals(&targets), targets } }

    fn resolve(&self, address: u64) -> Option<(&Target, i64)> {
        let i = self.intervals.range(..=address).next_back()?.1.as_ref()?;
        let target = &self.targets[*i];
        Some((target, (address - target.address) as i64))
    }
}

fn read_at<'a>(file: &'a object::File, addr: u64, size: u64) -> Option<&'a [u8]> {
    let mut matches = file.sections().filter(|s| matches!(s.flags(), object::SectionFlags::Elf { sh_flags } if sh_flags & u64::from(elf::SHF_ALLOC) != 0)).filter_map(|s| s.data_range(addr, size).ok().flatten());
    let data = matches.next()?;
    if matches.next().is_some() { None } else { Some(data) }
}

impl Linked {
    pub fn new(arch: &ArchX86, file: &object::File) -> Self {
        if file.format() != object::BinaryFormat::Elf
            || file.architecture() != object::Architecture::I386
            || file.kind() != object::ObjectKind::Dynamic
            || !file.is_little_endian()
        {
            return Self::default();
        }
        let got_ranges: Vec<_> = file
            .sections()
            .filter(|s| matches!(s.name(), Ok(".got" | ".got.plt")))
            .filter_map(|s| Some((s.address(), s.address().checked_add(s.size())?)))
            .collect();
        let mut bases = BTreeSet::new();
        for sym in file.symbols().chain(file.dynamic_symbols()) {
            if sym.name() == Ok("_GLOBAL_OFFSET_TABLE_") && sym.is_definition() {
                bases.insert(sym.address());
            }
        }
        // DT_PLTGOT is the ABI base even when _GLOBAL_OFFSET_TABLE_ is stripped.
        if let Some(section) = file.section_by_name(".dynamic")
            && let Ok(data) = section.data()
        {
            for entry in data.as_chunks::<8>().0 {
                let tag = u32::from_le_bytes(entry[..4].try_into().unwrap());
                if tag as i64 == elf::DT_NULL {
                    break;
                }
                if tag as i64 == elf::DT_PLTGOT {
                    bases.insert(u32::from_le_bytes(entry[4..].try_into().unwrap()) as u64);
                }
            }
        }
        if bases.is_empty()
            && let Some(s) = file.section_by_name(".got.plt")
        {
            bases.insert(s.address());
        }
        if bases.len() != 1 {
            return Self::default();
        }
        let base = *bases.first().unwrap();
        if !got_ranges.iter().any(|&(start, end)| base >= start && base < end) {
            return Self::default();
        }
        let targets: Vec<_> = file
            .symbols()
            .chain(file.dynamic_symbols())
            .filter_map(|s| target(file, &s))
            .filter(|t| t.section.is_some())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index = SymbolIndex::new(targets);
        let mut slots = BTreeMap::new();
        let mut seen = BTreeSet::new();
        if let Some(relocs) = file.dynamic_relocations() {
            for (addr, reloc) in relocs {
                if !got_ranges.iter().any(|&(start, end)| {
                    addr >= start && addr.checked_add(4).is_some_and(|a| a <= end)
                }) || addr % 4 != 0
                    || read_at(file, addr, 4).is_none()
                {
                    continue;
                }
                // Multiple relocations at one slot are never guessed, even if identical.
                if !seen.insert(addr) {
                    slots.remove(&addr);
                    continue;
                }
                let object::RelocationFlags::Elf { r_type } = reloc.flags() else { continue };
                let resolved = match r_type {
                    elf::R_386_RELATIVE
                        if matches!(reloc.target(), object::RelocationTarget::Absolute) =>
                    {
                        let value = if reloc.has_implicit_addend() {
                            read_at(file, addr, 4)
                                .and_then(|b| b.try_into().ok())
                                .map(u32::from_le_bytes)
                                .map(u64::from)
                        } else {
                            u32::try_from(reloc.addend()).ok().map(u64::from)
                        };
                        value.and_then(|v| index.resolve(v)).map(|(t, addend)| (t.clone(), addend))
                    }
                    elf::R_386_GLOB_DAT => {
                        if let object::RelocationTarget::Symbol(i) = reloc.target() {
                            file.dynamic_symbol_table()
                                .and_then(|table| table.symbol_by_index(i).ok())
                                .and_then(|s| target(file, &s))
                                .map(|t| (t, 0))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some((t, addend)) = resolved {
                    slots.insert(addr, RecoveredTarget::GotSlot {
                        name: t.name,
                        section: t.section,
                        addend,
                    });
                }
            }
        }
        let mut result = Self::default();
        let functions: BTreeSet<_> = file
            .symbols()
            .chain(file.dynamic_symbols())
            .filter(|s| s.kind() == object::SymbolKind::Text && s.is_definition() && s.size() > 0)
            .map(|s| (s.address(), s.size()))
            .collect();
        let functions: Vec<_> = functions.into_iter().collect();
        let mut thunks = BTreeMap::new();
        let mut previous_end = 0;
        for (n, &(addr, size)) in functions.iter().enumerate() {
            let end = addr.saturating_add(size);
            let overlaps =
                addr < previous_end || functions.get(n + 1).is_some_and(|&(next, _)| next < end);
            previous_end = previous_end.max(end);
            if overlaps {
                continue;
            }
            let Some(data) = read_at(file, addr, size) else { continue };
            let instructions: Vec<_> = arch.decoder(data, addr).into_iter().collect();
            // Reject malformed decoding and indirect jumps (unknown incoming edges).
            if instructions
                .iter()
                .any(|i| i.is_invalid() || i.flow_control() == FlowControl::IndirectBranch)
            {
                continue;
            }
            for i in &instructions {
                if i.code() == Code::Call_rel32_32 {
                    thunks.entry(i.near_branch_target()).or_insert_with(|| {
                        let mut address = i.near_branch_target();
                        for _ in 0..16 {
                            if read_at(file, address, 1)? != [0x90] {
                                break;
                            }
                            address = address.checked_add(1)?;
                        }
                        let data = read_at(file, address, 4)?;
                        let mut decoder = arch.decoder(data, address);
                        let mov = decoder.decode();
                        let ret = decoder.decode();
                        (mov.code() == Code::Mov_r32_rm32
                            && mov.op1_kind() == OpKind::Memory
                            && mov.memory_base() == Register::ESP
                            && mov.memory_index() == Register::None
                            && mov.memory_displacement64() == 0
                            && mov.segment_prefix() == Register::None
                            && ret.code() == Code::Retnd
                            && gpr(mov.op0_register()).is_some()
                            && mov.op0_register() != Register::ESP)
                            .then_some(mov.op0_register())
                    });
                }
            }
            result.analyze(addr, &instructions, base as u32, &slots, &thunks, &index);
        }
        result
    }

    fn analyze(
        &mut self,
        start: u64,
        instructions: &[Instruction],
        base: u32,
        slots: &BTreeMap<u64, RecoveredTarget>,
        thunks: &BTreeMap<u64, Option<Register>>,
        index: &SymbolIndex,
    ) {
        if instructions.is_empty() {
            return;
        }
        let addresses: BTreeMap<_, _> =
            instructions.iter().enumerate().map(|(n, i)| (i.ip(), n)).collect();
        let mut edges = vec![Vec::new(); instructions.len()];
        for (n, i) in instructions.iter().enumerate() {
            match i.flow_control() {
                FlowControl::Next | FlowControl::Call | FlowControl::IndirectCall => {
                    if n + 1 < instructions.len() {
                        edges[n].push(n + 1);
                    }
                }
                FlowControl::ConditionalBranch | FlowControl::UnconditionalBranch => {
                    if let Some(&dest) = addresses.get(&i.near_branch_target()) {
                        edges[n].push(dest);
                    } else if i.near_branch_target() >= start
                        && i.near_branch_target() < instructions.last().unwrap().next_ip()
                    {
                        return;
                    }
                    if i.flow_control() == FlowControl::ConditionalBranch
                        && n + 1 < instructions.len()
                    {
                        edges[n].push(n + 1);
                    }
                }
                _ => {}
            }
        }
        let mut states = vec![None; instructions.len()];
        states[0] = Some([Fact::Unknown; 8]);
        let mut queue = VecDeque::from([0]);
        let mut info = InstructionInfoFactory::new();
        while let Some(n) = queue.pop_front() {
            let state = transfer(&instructions[n], states[n].unwrap(), base, thunks, &mut info);
            for &dest in &edges[n] {
                let merged = match states[dest] {
                    None => state,
                    Some(old) => core::array::from_fn(|r| {
                        if old[r] == state[r] { old[r] } else { Fact::Unknown }
                    }),
                };
                if states[dest] != Some(merged) {
                    states[dest] = Some(merged);
                    queue.push_back(dest);
                }
            }
        }
        for (n, i) in instructions.iter().enumerate() {
            let Some(state) = states[n] else { continue };
            if got_add(i, &state, base).is_some() && flags_dead(n, instructions, &edges, thunks) {
                self.references.insert((start, i.ip()), RecoveredReference {
                    target: RecoveredTarget::GotBase,
                    raw_value: i.immediate32(),
                });
            }
            // Pointer loads and direct address calculations have different symbolic semantics.
            if !matches!(i.code(), Code::Mov_r32_rm32 | Code::Lea_r32_m)
                || i.op1_kind() != OpKind::Memory
                || i.memory_index() != Register::None
                || i.segment_prefix() != Register::None
                || !gpr(i.memory_base()).is_some_and(|r| state[r] == Fact::Got)
            {
                continue;
            }
            let raw_value = i.memory_displacement32();
            let address = base.wrapping_add(raw_value) as u64;
            let target = match i.code() {
                Code::Mov_r32_rm32 => slots.get(&address).cloned(),
                Code::Lea_r32_m => {
                    index.resolve(address).map(|(symbol, addend)| RecoveredTarget::GotRelative {
                        name: symbol.name.clone(),
                        section: symbol.section.clone(),
                        addend,
                    })
                }
                _ => None,
            };
            if let Some(target) = target {
                self.references.insert((start, i.ip()), RecoveredReference { target, raw_value });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fact {
    Unknown,
    Pc(u32),
    Got,
}

fn gpr(reg: Register) -> Option<usize> {
    match reg {
        Register::EAX => Some(0),
        Register::ECX => Some(1),
        Register::EDX => Some(2),
        Register::EBX => Some(3),
        Register::ESP => Some(4),
        Register::EBP => Some(5),
        Register::ESI => Some(6),
        Register::EDI => Some(7),
        _ => None,
    }
}

fn got_add(i: &Instruction, state: &[Fact; 8], base: u32) -> Option<usize> {
    if !matches!(i.code(), Code::Add_rm32_imm32 | Code::Add_EAX_imm32)
        || i.op0_kind() != OpKind::Register
    {
        return None;
    }
    let r = gpr(i.op0_register())?;
    if let Fact::Pc(pc) = state[r]
        && pc.wrapping_add(i.immediate32()) == base
    {
        Some(r)
    } else {
        None
    }
}

fn transfer(
    i: &Instruction,
    mut state: [Fact; 8],
    base: u32,
    thunks: &BTreeMap<u64, Option<Register>>,
    factory: &mut InstructionInfoFactory,
) -> [Fact; 8] {
    let old = state;
    for reg in factory.info(i).used_registers() {
        if matches!(
            reg.access(),
            OpAccess::Write | OpAccess::CondWrite | OpAccess::ReadWrite | OpAccess::ReadCondWrite
        ) && let Some(r) = gpr(reg.register().full_register32())
        {
            state[r] = Fact::Unknown;
        }
    }
    if matches!(i.flow_control(), FlowControl::Call | FlowControl::IndirectCall) {
        // SysV i386 caller-saved registers. PC facts never survive unrelated instructions.
        for value in &mut state {
            if matches!(value, Fact::Pc(_)) {
                *value = Fact::Unknown;
            }
        }
        state[0] = Fact::Unknown;
        state[1] = Fact::Unknown;
        state[2] = Fact::Unknown;
        if i.code() == Code::Call_rel32_32
            && let Some(Some(reg)) = thunks.get(&i.near_branch_target())
        {
            state[gpr(*reg).unwrap()] = Fact::Pc(i.next_ip32());
        }
        return state;
    }
    for value in &mut state {
        if matches!(value, Fact::Pc(_)) {
            *value = Fact::Unknown;
        }
    }
    if let Some(r) = got_add(i, &old, base) {
        state[r] = Fact::Got;
    }
    if matches!(i.code(), Code::Mov_r32_rm32 | Code::Mov_rm32_r32)
        && i.op0_kind() == OpKind::Register
        && i.op1_kind() == OpKind::Register
        && let (Some(dest), Some(src)) = (gpr(i.op0_register()), gpr(i.op1_register()))
        && old[src] == Fact::Got
    {
        state[dest] = Fact::Got;
    }
    state
}

/// ADD also sets flags. Normalize its immediate only if no dependent flag is read.
fn flags_dead(
    n: usize,
    instructions: &[Instruction],
    edges: &[Vec<usize>],
    thunks: &BTreeMap<u64, Option<Register>>,
) -> bool {
    let mut queue: Vec<_> =
        edges[n].iter().map(|&dest| (dest, instructions[n].rflags_modified())).collect();
    let mut visited = BTreeSet::new();
    while let Some((n, flags)) = queue.pop() {
        if !visited.insert((n, flags)) {
            continue;
        }
        let i = &instructions[n];
        if i.rflags_read() & flags != 0 {
            return false;
        }
        if matches!(i.flow_control(), FlowControl::Call | FlowControl::IndirectCall)
            && !(i.code() == Code::Call_rel32_32
                && thunks.get(&i.near_branch_target()).is_some_and(Option::is_some))
        {
            continue;
        }
        // Validated PC thunks preserve flags, unlike a general ABI call boundary.
        let flags = flags & !i.rflags_modified();
        if flags != 0 {
            queue.extend(edges[n].iter().map(|&dest| (dest, flags)));
        }
    }
    true
}
