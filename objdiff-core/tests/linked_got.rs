#![cfg(feature = "x86")]
use objdiff_core::{
    diff::{self, DiffObjConfig, DiffSide, InstructionDiffKind},
    obj::{self, InstructionArg, RecoveredTarget},
};
mod common;

fn load(right: bool, enabled: bool) -> (obj::Object, DiffObjConfig) {
    let config = DiffObjConfig { x86_recover_linked_got: enabled, ..Default::default() };
    let data = if right {
        include_object!("data/x86/linked-got/right.elf")
    } else {
        include_object!("data/x86/linked-got/left.elf")
    };
    (obj::read::parse(data, &config, DiffSide::Base).unwrap(), config)
}
fn args(object: &obj::Object, config: &DiffObjConfig, name: &str) -> Vec<obj::ParsedInstruction> {
    let i = object.symbol_by_name(name).unwrap();
    diff::code::no_diff_code(object, i, config)
        .unwrap()
        .instruction_rows
        .iter()
        .map(|row| {
            object
                .arch
                .process_instruction(
                    object.resolve_instruction_ref(i, row.ins_ref.unwrap()).unwrap(),
                    config,
                )
                .unwrap()
        })
        .collect()
}
fn refs(object: &obj::Object, config: &DiffObjConfig, name: &str) -> Vec<obj::RecoveredReference> {
    args(object, config, name)
        .into_iter()
        .flat_map(|i| i.args)
        .filter_map(|a| match a {
            InstructionArg::Recovered(r) => Some(r),
            _ => None,
        })
        .collect()
}

#[test]
fn equivalent_references_and_display() {
    let (left, config) = load(false, true);
    let (right, _) = load(true, true);
    for name in [
        "relative",
        "named",
        "interior",
        "non_ebx",
        "accumulator",
        "copied",
        "loop_equal",
        "callee_saved",
    ] {
        let l = refs(&left, &config, name);
        let r = refs(&right, &config, name);
        assert_eq!(l.len(), 2, "{name}: {l:?}");
        assert_eq!(r.len(), 2, "{name}: {r:?}");
        assert!(l.iter().zip(&r).all(|(a, b)| a.matches(b)), "{name}");
        assert_ne!(l[0].raw_value, r[0].raw_value, "{name}");
        if name != "named" {
            assert_ne!(l[1].raw_value, r[1].raw_value, "{name}");
        }
        let li = left.symbol_by_name(name).unwrap();
        let ri = right.symbol_by_name(name).unwrap();
        let (ld, _) = diff::code::diff_code(&left, &right, li, ri, &config).unwrap();
        assert!(
            ld.instruction_rows.iter().all(|r| r.kind == InstructionDiffKind::None),
            "{name}: {ld:?}"
        );
        let display = common::display_diff(&left, &ld, li, &config);
        assert!(display.contains("GOT_BASE"), "{display}");
        assert!(display.contains("GOT("), "{display}");
    }
    assert!(
        matches!(&refs(&left, &config, "named")[1].target, RecoveredTarget::GotSlot { name, section: None, addend: 0 } if name == "external")
    );
    assert!(
        matches!(&refs(&left, &config, "interior")[1].target, RecoveredTarget::GotSlot { name, addend: 2, .. } if name == "object")
    );
    assert_ne!(
        left.symbols[left.symbol_by_name("object").unwrap()].size,
        right.symbols[right.symbol_by_name("object").unwrap()].size
    );
    let result =
        diff::diff_objs(Some(&left), Some(&right), None, &config, &Default::default()).unwrap();
    assert_ne!(
        result.left.unwrap().symbols[left.symbol_by_name("object").unwrap()].match_percent,
        Some(100.0)
    );
}

#[test]
fn real_changes_remain_mismatches() {
    let (left, config) = load(false, true);
    let (right, _) = load(true, true);
    for name in [
        "different_symbol",
        "different_addend",
        "different_register",
        "different_width",
        "different_opcode",
    ] {
        let (ld, _) = diff::code::diff_code(
            &left,
            &right,
            left.symbol_by_name(name).unwrap(),
            right.symbol_by_name(name).unwrap(),
            &config,
        )
        .unwrap();
        assert!(ld.instruction_rows.iter().any(|r| r.kind != InstructionDiffKind::None), "{name}");
    }
}

#[test]
fn conservative_fallbacks() {
    let (left, config) = load(false, true);
    for name in [
        "overwritten",
        "partial_write",
        "conflicting",
        "indexed",
        "tls_segment",
        "caller_clobber",
        "ambiguous",
        "overlapping",
        "missing",
        "unsupported",
    ] {
        let references = refs(&left, &config, name);
        assert!(
            references.iter().all(|r| r.target == RecoveredTarget::GotBase),
            "{name}: {references:?}"
        );
        assert!(!references.is_empty(), "{name}");
    }
    for name in ["numeric", "unproven", "indirect_jump"] {
        assert!(refs(&left, &config, name).is_empty(), "{name}");
    }
    assert!(refs(&left, &config, "flag_user").iter().all(|r| r.target != RecoveredTarget::GotBase));
    assert!(
        refs(&left, &config, "flag_through_thunk")
            .iter()
            .all(|r| r.target != RecoveredTarget::GotBase)
    );
}

#[test]
fn disabled_and_relocatable_unchanged() {
    let (left, config) = load(false, false);
    let (right, _) = load(true, false);
    assert!(refs(&left, &config, "relative").is_empty());
    let (ld, _) = diff::code::diff_code(
        &left,
        &right,
        left.symbol_by_name("relative").unwrap(),
        right.symbol_by_name("relative").unwrap(),
        &config,
    )
    .unwrap();
    assert_eq!(ld.instruction_rows[1].kind, InstructionDiffKind::ArgMismatch);
    assert_eq!(ld.instruction_rows[2].kind, InstructionDiffKind::ArgMismatch);
    let enabled = DiffObjConfig { x86_recover_linked_got: true, ..Default::default() };
    let bytes = include_object!("data/x86/linked-got/relocatable.o");
    let a = obj::read::parse(bytes, &config, DiffSide::Target).unwrap();
    let b = obj::read::parse(bytes, &enabled, DiffSide::Base).unwrap();
    assert_eq!(
        format!("{:?}", args(&a, &config, "relative")),
        format!("{:?}", args(&b, &enabled, "relative"))
    );
    let (ld, _) = diff::code::diff_code(
        &a,
        &b,
        a.symbol_by_name("relative").unwrap(),
        b.symbol_by_name("relative").unwrap(),
        &enabled,
    )
    .unwrap();
    assert_eq!(ld.match_percent, Some(100.0));
}

// Mutate the tiny linked fixture in memory; CI needs neither binutils nor game assets.
fn mutated(edit: impl FnOnce(&object::File<'_>, &mut Vec<u8>)) -> obj::Object {
    let source = include_object!("data/x86/linked-got/left.elf");
    let file = object::File::parse(source).unwrap();
    let mut data = source.to_vec();
    edit(&file, &mut data);
    obj::read::parse(
        &data,
        &DiffObjConfig { x86_recover_linked_got: true, ..Default::default() },
        DiffSide::Base,
    )
    .unwrap()
}
fn symbol_offset(file: &object::File<'_>, name: &str) -> usize {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};
    let symbol = file.symbol_by_name(name).unwrap();
    let section = file.section_by_index(symbol.section_index().unwrap()).unwrap();
    (section.file_range().unwrap().0 + symbol.address() - section.address()) as usize
}

#[test]
fn identical_bytes_still_compare_symbol_identity() {
    use object::{Object as _, ObjectSymbol as _};
    let (left, config) = load(false, true);
    let right = mutated(|file, data| {
        let offset = symbol_offset(file, "slot");
        let address = file.symbol_by_name("other").unwrap().address() as u32;
        data[offset..offset + 4].copy_from_slice(&address.to_le_bytes());
    });
    for name in ["relative", "add_slot"] {
        assert_ne!(refs(&left, &config, name)[1].target, refs(&right, &config, name)[1].target);
        let li = left.symbol_by_name(name).unwrap();
        let ri = right.symbol_by_name(name).unwrap();
        let (ld, _) = diff::code::diff_code(&left, &right, li, ri, &config).unwrap();
        assert_eq!(ld.instruction_rows[2].kind, InstructionDiffKind::ArgMismatch);
        assert_eq!(left.symbol_data(li), right.symbol_data(ri));
    }
}

#[test]
fn malformed_and_unresolvable_metadata_falls_back() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};
    let config = DiffObjConfig { x86_recover_linked_got: true, ..Default::default() };
    for mode in 0..5 {
        let obj = mutated(|file, data| {
            if mode == 0 {
                let offset = symbol_offset(file, "slot");
                data[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            } else if mode == 1 || mode == 2 {
                let rel = file.section_by_name(".rel.dyn").unwrap();
                let offset = rel.file_range().unwrap().0 as usize;
                let slot = file.symbol_by_name("slot").unwrap().address() as u32;
                let entry = (offset..offset + rel.size() as usize)
                    .step_by(8)
                    .find(|&o| u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) == slot)
                    .unwrap();
                if mode == 1 {
                    data[entry..entry + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                } else {
                    data[entry + 4..entry + 8].copy_from_slice(&0x7fu32.to_le_bytes());
                }
            } else {
                let sym = file.symbol_by_name("object").unwrap();
                let offset = file.section_by_name(".symtab").unwrap().file_range().unwrap().0
                    as usize
                    + sym.index().0 * 16;
                if mode == 3 {
                    data[offset..offset + 4].fill(0);
                } else {
                    data[offset + 8..offset + 12].copy_from_slice(&u32::MAX.to_le_bytes());
                }
            }
        });
        assert!(
            refs(&obj, &config, "relative").iter().all(|r| r.target == RecoveredTarget::GotBase),
            "mode {mode}"
        );
    }
    let source = include_object!("data/x86/linked-got/left.elf");
    for length in [0, 4, 16, 40, source.len() - 1] {
        assert!(obj::read::parse(&source[..length], &config, DiffSide::Base).is_err());
    }
}

#[test]
fn symbol_order_and_missing_got_symbol() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};
    let (left, config) = load(false, true);
    let right = mutated(|file, data| {
        let section = file.section_by_name(".symtab").unwrap();
        let offset = section.file_range().unwrap().0 as usize;
        let got = file.symbol_by_name("_GLOBAL_OFFSET_TABLE_").unwrap();
        data[offset + got.index().0 * 16..offset + got.index().0 * 16 + 4].fill(0);
        let records: Vec<_> = data[offset + 16..offset + section.size() as usize]
            .as_chunks::<16>()
            .0
            .iter()
            .map(|x| x.to_vec())
            .rev()
            .collect();
        for (out, record) in data[offset + 16..offset + section.size() as usize]
            .as_chunks_mut::<16>()
            .0
            .iter_mut()
            .zip(records)
        {
            out.copy_from_slice(&record);
        }
    });
    assert_eq!(refs(&left, &config, "relative"), refs(&right, &config, "relative"));
}

#[test]
fn exported_recovery_retains_raw_operand_and_bytes() {
    let (object, config) = load(false, true);
    let index = object.symbol_by_name("relative").unwrap();
    let diff = diff::code::no_diff_code(&object, index, &config).unwrap();
    let ins_ref = diff.instruction_rows[2].ins_ref.unwrap();
    let resolved = object.resolve_instruction_ref(index, ins_ref).unwrap();
    let exported =
        objdiff_core::bindings::diff::DiffInstruction::new(&object, resolved, &config).unwrap();
    assert_eq!(exported.raw_bytes, resolved.code);
    assert!(exported.relocation.is_none());
    let metadata = exported
        .parts
        .iter()
        .find_map(|part| {
            if let Some(objdiff_core::bindings::diff::diff_instruction_part::Part::Arg(arg)) =
                &part.part
            {
                arg.recovered.as_ref()
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(metadata.symbol.as_deref(), Some("object"));
    assert_eq!(metadata.raw_value, refs(&object, &config, "relative")[1].raw_value);
}

#[test]
fn all_formatters_emit_symbolic_operands_and_raw_hover() {
    let (object, mut config) = load(false, true);
    for formatter in [
        diff::X86Formatter::Intel,
        diff::X86Formatter::Gas,
        diff::X86Formatter::Masm,
        diff::X86Formatter::Nasm,
    ] {
        config.x86_formatter = formatter;
        for name in ["relative", "add_slot"] {
            assert_eq!(refs(&object, &config, name).len(), 2, "{formatter:?}: {name}");
        }
    }
    let index = object.symbol_by_name("relative").unwrap();
    let instructions = args(&object, &config, "relative");
    let ins = &instructions[2];
    let resolved = object.resolve_instruction_ref(index, ins.ins_ref).unwrap();
    let hover = diff::display::instruction_hover(&object, resolved, ins, &config);
    assert!(hover.iter().any(|h| matches!(h, diff::display::HoverItem::Text { label, .. } if label == "Raw linked operand")));
    let context = diff::display::instruction_context(&object, resolved, ins, &config);
    assert!(context.iter().any(|c| matches!(c, diff::display::ContextItem::Copy { label: Some(label), .. } if label == "raw linked operand")));
}

#[test]
fn invalid_code_and_conflicting_base_metadata() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};
    let config = DiffObjConfig { x86_recover_linked_got: true, ..Default::default() };
    let invalid = mutated(|file, data| {
        let sym = file.symbol_by_name("relative").unwrap();
        data[symbol_offset(file, "relative") + sym.size() as usize - 1] = 0x0f;
    });
    assert!(refs(&invalid, &config, "relative").is_empty());
    let conflicting = mutated(|file, data| {
        let got = file.symbol_by_name("_GLOBAL_OFFSET_TABLE_").unwrap();
        let offset = file.section_by_name(".symtab").unwrap().file_range().unwrap().0 as usize
            + got.index().0 * 16
            + 4;
        data[offset..offset + 4].copy_from_slice(&(got.address() as u32 + 4).to_le_bytes());
    });
    assert!(refs(&conflicting, &config, "relative").is_empty());
}

#[test]
fn got_relative_direct_addresses() {
    let (left, config) = load(false, true);
    let (right, _) = load(true, true);
    for name in [
        "direct_address",
        "direct_interior",
        "direct_non_ebx",
        "direct_negative",
        "direct_zero",
        "direct_copied",
    ] {
        let l = refs(&left, &config, name);
        let r = refs(&right, &config, name);
        assert_eq!(l.len(), 2, "{name}: {l:?}");
        assert_eq!(r.len(), 2, "{name}: {r:?}");
        assert!(matches!(l[1].target, RecoveredTarget::GotRelative { .. }));
        assert!(l[1].matches(&r[1]), "{name}");
        if name != "direct_zero" {
            assert_ne!(l[1].raw_value, r[1].raw_value, "{name}");
        }
        let li = left.symbol_by_name(name).unwrap();
        let ri = right.symbol_by_name(name).unwrap();
        let (ld, _) = diff::code::diff_code(&left, &right, li, ri, &config).unwrap();
        assert_eq!(ld.match_percent, Some(100.0), "{name}: {ld:?}");
        assert!(common::display_diff(&left, &ld, li, &config).contains("GOTOFF("));
    }
    assert!(matches!(&refs(&left, &config, "direct_interior")[1].target,
        RecoveredTarget::GotRelative { name, addend: 2, .. } if name == "object"));
    // A pointer load and a direct address must not be confused even for the same symbol.
    assert!(
        !refs(&left, &config, "relative")[1].matches(&refs(&left, &config, "direct_address")[1])
    );
    let instructions = args(&left, &config, "direct_kills_base");
    assert!(instructions[2].args.iter().any(|a| matches!(a, InstructionArg::Recovered(_))));
    assert!(!instructions[3].args.iter().any(|a| matches!(a, InstructionArg::Recovered(_))));
}

#[test]
fn direct_address_changes_and_fallbacks() {
    let (left, config) = load(false, true);
    let (right, _) = load(true, true);
    for name in [
        "direct_different_symbol",
        "direct_different_addend",
        "direct_different_register",
        "direct_different_opcode",
        "direct_different_width",
    ] {
        let (ld, _) = diff::code::diff_code(
            &left,
            &right,
            left.symbol_by_name(name).unwrap(),
            right.symbol_by_name(name).unwrap(),
            &config,
        )
        .unwrap();
        assert_ne!(ld.instruction_rows[2].kind, InstructionDiffKind::None, "{name}");
    }
    for name in [
        "direct_clobber",
        "direct_indexed",
        "direct_conflicting",
        "direct_alias",
        "direct_overlap",
        "direct_missing",
    ] {
        assert!(
            refs(&left, &config, name).iter().all(|r| r.target == RecoveredTarget::GotBase),
            "{name}"
        );
    }
}

#[test]
fn recovery_defaults_on_and_direct_addresses_can_be_disabled() {
    let default = DiffObjConfig::default();
    assert!(default.x86_recover_linked_got);
    let left = obj::read::parse(
        include_object!("data/x86/linked-got/left.elf"),
        &default,
        DiffSide::Target,
    )
    .unwrap();
    assert!(matches!(
        refs(&left, &default, "direct_address")[1].target,
        RecoveredTarget::GotRelative { .. }
    ));
    let (left, disabled) = load(false, false);
    let (right, _) = load(true, false);
    assert!(refs(&left, &disabled, "direct_address").is_empty());
    let (ld, _) = diff::code::diff_code(
        &left,
        &right,
        left.symbol_by_name("direct_address").unwrap(),
        right.symbol_by_name("direct_address").unwrap(),
        &disabled,
    )
    .unwrap();
    assert_eq!(ld.instruction_rows[2].kind, InstructionDiffKind::ArgMismatch);
}

#[test]
fn direct_address_formatting_and_export() {
    let (object, mut config) = load(false, true);
    for formatter in [
        diff::X86Formatter::Intel,
        diff::X86Formatter::Gas,
        diff::X86Formatter::Masm,
        diff::X86Formatter::Nasm,
    ] {
        config.x86_formatter = formatter;
        for name in ["direct_address", "direct_negative", "direct_zero"] {
            assert_eq!(refs(&object, &config, name).len(), 2, "{formatter:?}: {name}");
        }
    }
    let index = object.symbol_by_name("direct_interior").unwrap();
    let ins = &args(&object, &config, "direct_interior")[2];
    let resolved = object.resolve_instruction_ref(index, ins.ins_ref).unwrap();
    let exported =
        objdiff_core::bindings::diff::DiffInstruction::new(&object, resolved, &config).unwrap();
    assert_eq!(exported.raw_bytes, resolved.code);
    assert!(exported.relocation.is_none());
    let metadata = exported
        .parts
        .iter()
        .find_map(|p| match &p.part {
            Some(objdiff_core::bindings::diff::diff_instruction_part::Part::Arg(a)) => {
                a.recovered.as_ref()
            }
            _ => None,
        })
        .unwrap();
    assert!(metadata.got_relative);
    assert!(!metadata.got_base);
    assert_eq!(metadata.symbol.as_deref(), Some("object"));
    assert_eq!(metadata.addend, 2);
}

#[test]
fn identical_lea_bytes_still_check_direct_symbol_identity() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};
    let (left, config) = load(false, true);
    let right = mutated(|file, data| {
        let table = file.section_by_name(".symtab").unwrap().file_range().unwrap().0 as usize;
        let object = file.symbol_by_name("object").unwrap();
        let other = file.symbol_by_name("other").unwrap();
        let a = table + object.index().0 * 16 + 4;
        let b = table + other.index().0 * 16 + 4;
        data[a..a + 4].copy_from_slice(&(other.address() as u32).to_le_bytes());
        data[b..b + 4].copy_from_slice(&(object.address() as u32).to_le_bytes());
    });
    let li = left.symbol_by_name("direct_address").unwrap();
    let ri = right.symbol_by_name("direct_address").unwrap();
    assert_eq!(left.symbol_data(li), right.symbol_data(ri));
    let (ld, _) = diff::code::diff_code(&left, &right, li, ri, &config).unwrap();
    assert_eq!(ld.instruction_rows[2].kind, InstructionDiffKind::ArgMismatch);
}

#[test]
fn add_got_slots_preserve_arithmetic_and_register_tracking() {
    let (left, config) = load(false, true);
    let (right, _) = load(true, true);
    for name in ["add_slot", "add_named", "add_interior", "add_copied"] {
        let l = refs(&left, &config, name);
        let r = refs(&right, &config, name);
        assert_eq!(l.len(), 2, "{name}");
        assert_eq!(r.len(), 2, "{name}");
        assert!(matches!(l[1].target, RecoveredTarget::GotSlot { .. }));
        assert!(l[1].matches(&r[1]));
        let li = left.symbol_by_name(name).unwrap();
        let (ld, _) =
            diff::code::diff_code(&left, &right, li, right.symbol_by_name(name).unwrap(), &config)
                .unwrap();
        assert_eq!(ld.match_percent, Some(100.0), "{name}");
        assert!(common::display_diff(&left, &ld, li, &config).contains("GOT("));
    }
    let ins = args(&left, &config, "add_kills_base");
    assert!(ins[2].args.iter().any(|a| matches!(a, InstructionArg::Recovered(_))));
    assert!(!ins[3].args.iter().any(|a| matches!(a, InstructionArg::Recovered(_))));
    for name in ["add_overwritten", "add_indexed", "add_store", "add_direct"] {
        assert!(
            refs(&left, &config, name).iter().all(|r| r.target == RecoveredTarget::GotBase),
            "{name}"
        );
    }
    for name in [
        "add_different_symbol",
        "add_different_addend",
        "add_different_register",
        "add_different_opcode",
        "add_different_width",
    ] {
        let (ld, _) = diff::code::diff_code(
            &left,
            &right,
            left.symbol_by_name(name).unwrap(),
            right.symbol_by_name(name).unwrap(),
            &config,
        )
        .unwrap();
        assert_ne!(ld.instruction_rows[2].kind, InstructionDiffKind::None, "{name}");
    }
    let (left, disabled) = load(false, false);
    let (right, _) = load(true, false);
    assert!(refs(&left, &disabled, "add_slot").is_empty());
    let (ld, _) = diff::code::diff_code(
        &left,
        &right,
        left.symbol_by_name("add_slot").unwrap(),
        right.symbol_by_name("add_slot").unwrap(),
        &disabled,
    )
    .unwrap();
    assert_eq!(ld.instruction_rows[2].kind, InstructionDiffKind::ArgMismatch);
}
