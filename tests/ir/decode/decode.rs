use super::*;

fn d(bytes: &[u8], mode32: bool) -> DecodedInstruction {
    decode(bytes, GuestEip(0xFFFE), LinearAddress(0x1FFFE), mode32).unwrap()
}

#[test]
fn prefixes_boundaries_and_missing_page() {
    let i = d(&[0x66, 0x66, 0x67, 0x64, 0x65, 0xB8, 0x34, 0x12], true);
    assert_eq!(i.length, 8);
    assert_eq!(i.operand_size, 16);
    assert_eq!(i.address_size, 16);
    assert_eq!(i.prefixes.segment, Some(5));
    assert_eq!(i.immediate, Some(0x1234));
    assert_eq!(i.next_pc, GuestEip(0x10006));
    assert_eq!(
        decode(&[0xB8, 1], GuestEip(4095), LinearAddress(4095), true).unwrap_err(),
        DecodeStop::Incomplete { available: 2 }
    );
    assert_eq!(d(&[0xB8, 1, 2, 3, 4], true).length, 5);
    let mut bytes = vec![0x66; 14];
    bytes.push(0x90);
    assert_eq!(d(&bytes, true).length, 15);
    bytes.insert(0, 0x66);
    assert_eq!(
        decode(&bytes, GuestEip(0), LinearAddress(0), true).unwrap_err(),
        DecodeStop::TooLong
    );
    assert_eq!(
        d(&[0xF3, 0xF2, 0x0F, 0x10, 0xC0], true).encoding.opcode,
        0xF20F10
    );
    assert_eq!(
        d(&[0xF2, 0x66, 0x0F, 0x10, 0xC0], true).encoding.opcode,
        0x660F10
    );
    assert!(d(&[0xF0, 0x01, 0x00], true).prefixes.lock);
    assert!(d(&[0x8D, 0xC0], true).baseline_ud);
    assert_eq!(d(&[0x0F, 0x20, 0x04], true).length, 3); // ignore_mod, no SIB
    assert_eq!(
        d(&[0x9A, 1, 2, 3, 4, 5, 6], true).extra_immediate,
        Some(0x605)
    );
    assert_eq!(d(&[0xC8, 1, 2, 3], true).extra_immediate, Some(3));
}

#[test]
fn ea_and_wrapping() {
    let gpr = [0xFFFFFFFF, 0x1111, 0x2222, 0xFFFF, 0x100, 0xFFFE, 3, 5];
    let i = d(&[0x8B, 0x44, 0x88, 0xFC], true); // [eax + ecx*4 - 4]
    let ea = i.ea.unwrap();
    assert_eq!(ea.offset(&gpr), 0x443F);
    assert_eq!(ea.segment, 3);
    let i = d(&[0x8B, 0x46, 0x04], false); // [bp+4], 16-bit wrap
    assert_eq!(i.ea.unwrap().offset(&gpr), 2);
    assert_eq!(i.ea.unwrap().segment, 2);
    let i = d(&[0x8B, 0x04, 0xED, 1, 0, 0, 0], true); // [ebp*8 + 1], no base => DS
    assert_eq!(i.ea.unwrap().segment, 3);
    assert_eq!(i.ea.unwrap().offset(&gpr), 0x7FFF1);
    let i = d(&[0x8B, 0x06, 0xFF, 0xFF], false);
    assert_eq!(i.ea.unwrap().offset(&gpr), 65535);
    assert_eq!(i.ea.unwrap().segment, 3);
}

#[test]
fn catalogue_lengths_and_all_modrm_sib_forms() {
    let mut count = 0;
    let mut oracle_records = Vec::new();
    for encoding in encodings() {
        for mode32 in [false, true] {
            for m in 0..if encoding.fetch_modrm { 256 } else { 1 } {
                if encoding.group >= 0 && (m >> 3 & 7) != encoding.group as u32 {
                    continue;
                }
                let mut bytes = Vec::new();
                if encoding.opcode > 0xFFFF {
                    bytes.push((encoding.opcode >> 16) as u8);
                }
                if encoding.opcode > 0xFF {
                    bytes.push((encoding.opcode >> 8) as u8);
                }
                bytes.push(encoding.opcode as u8);
                let op_size32 = mode32 != bytes.contains(&0x66);
                if encoding.fetch_modrm {
                    bytes.push(m as u8);
                }
                let sib_count =
                    if mode32 && encoding.e && !encoding.ignore_mod && m < 0xC0 && m & 7 == 4 {
                        256
                    } else {
                        1
                    };
                for sib in 0..sib_count {
                    let mut sample = bytes.clone();
                    let mut displacement = 0;
                    if encoding.e && !encoding.ignore_mod && m < 0xC0 {
                        if mode32 && m & 7 == 4 {
                            sample.push(sib as u8);
                        }
                        displacement = match m >> 6 {
                            1 => 1,
                            2 => {
                                if mode32 {
                                    4
                                } else {
                                    2
                                }
                            },
                            _ if mode32 && (m & 7 == 5 || m & 7 == 4 && sib & 7 == 5) => 4,
                            _ if !mode32 && m & 7 == 6 => 2,
                            _ => 0,
                        };
                    }
                    let imm = match encoding.immediate {
                        ImmediateKind::None => 0,
                        ImmediateKind::Byte | ImmediateKind::SignedByte => 1,
                        ImmediateKind::Word => 2,
                        ImmediateKind::Operand => {
                            if op_size32 {
                                4
                            } else {
                                2
                            }
                        },
                        ImmediateKind::Address => {
                            if mode32 {
                                4
                            } else {
                                2
                            }
                        },
                    };
                    sample.resize(
                        sample.len() + displacement + imm + encoding.extra_bytes as usize,
                        0x25,
                    );
                    let decoded = d(&sample, mode32);
                    assert_eq!(
                        decoded.encoding.id, encoding.id,
                        "{:x} {m:x}",
                        encoding.opcode
                    );
                    assert_eq!(
                        decoded.length as usize,
                        sample.len(),
                        "{:x} {m:x}",
                        encoding.opcode
                    );
                    for cut in 0..sample.len() {
                        assert!(
                            decode(&sample[..cut], GuestEip(0), LinearAddress(0), mode32).is_err()
                        );
                    }
                    oracle_records.push(mode32 as u8);
                    oracle_records.push(sample.len() as u8);
                    oracle_records.extend_from_slice(&sample);
                    oracle_records.resize(oracle_records.len() + 15 - sample.len(), 0);
                    count += 1;
                }
            }
        }
    }
    // Repeat, conflicting and address/segment prefixes against every catalogue
    // opcode in both modes. Padding allows prefix selection to change its form.
    let prefix_sets: &[&[u8]] = &[&[0x66], &[0x67], &[0x66, 0x67], &[0xF0], &[0xF2], &[0xF3],
        &[0xF2, 0xF3], &[0xF3, 0xF2], &[0x66, 0xF2, 0xF3], &[0xF3, 0x66, 0xF2],
        &[0x26, 0x36, 0x64, 0x65], &[0x66, 0x66, 0x67, 0x67], &[0xF3, 0xF3]];
    for encoding in encodings() { for mode32 in [false, true] { for prefixes in prefix_sets { for memory in [false, true] {
        let mut sample = prefixes.to_vec();
        if encoding.opcode > 0xFFFF { sample.push((encoding.opcode >> 16) as u8); }
        if encoding.opcode > 0xFF { sample.push((encoding.opcode >> 8) as u8); }
        sample.push(encoding.opcode as u8);
        if encoding.fetch_modrm {
            sample.push((if memory { 4 } else { 0xC0 }) | (encoding.group.max(0) as u8) << 3);
            if memory { sample.push(0x9D); }
        }
        sample.resize(15, 0x25);
        if let Ok(decoded) = decode(&sample, GuestEip(0), LinearAddress(0), mode32) {
            oracle_records.push(mode32 as u8); oracle_records.push(decoded.length);
            oracle_records.extend_from_slice(&decoded.bytes); count += 1;
        }
    } } } }
    std::fs::create_dir_all("build/ir-decode").unwrap();
    std::fs::write("build/ir-decode/legacy-forms.bin", oracle_records).unwrap();
    assert!(count > 1_000_000);
    println!("Checked {count} encoding/ModRM/SIB forms and all truncated prefixes");
}

#[test]
fn independent_disassembler_corpus() {
    use std::fs;
    fs::create_dir_all("build/ir-decode").unwrap();
    for mode32 in [false, true] {
        let mut bytes = Vec::new();
        let mut starts = Vec::new();
        let mut seed = 0x12345678u32;
        for i in 0..8192 {
            let mut instruction = Vec::new();
            // Restrict oracle corpus to instructions NDISASM and the baseline both implement.
            if i % 5 == 0 {
                instruction.push(0x66);
            }
            if i % 7 == 0 {
                instruction.push(0x67);
            }
            if i % 11 == 0 {
                instruction.push(0x64);
            }
            instruction.push([0x8B, 0x89, 0x01, 0x29, 0x31, 0x39][i % 6]);
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let m = (seed >> 16) as u8;
            instruction.push(m);
            let size32 = mode32 != instruction.contains(&0x67);
            let sib = seed as u8;
            let mut displacement = 0;
            if m < 0xC0 {
                if size32 && m & 7 == 4 {
                    instruction.push(sib);
                }
                displacement = match m >> 6 {
                    1 => 1,
                    2 => {
                        if size32 {
                            4
                        } else {
                            2
                        }
                    },
                    _ if size32 && (m & 7 == 5 || m & 7 == 4 && sib & 7 == 5) => 4,
                    _ if !size32 && m & 7 == 6 => 2,
                    _ => 0,
                };
            }
            instruction.extend_from_slice(&seed.to_le_bytes()[..displacement]);
            assert_eq!(d(&instruction, mode32).length as usize, instruction.len());
            starts.push(bytes.len().to_string());
            bytes.extend(instruction);
        }
        let size = if mode32 { 32 } else { 16 };
        fs::write(format!("build/ir-decode/oracle-{size}.bin"), bytes).unwrap();
        fs::write(
            format!("build/ir-decode/oracle-{size}.json"),
            format!("[{}]", starts.join(",")),
        )
        .unwrap();
    }
}

#[test]
fn legacy_dispatch_boundary_and_invalid_form_fetch() {
    // A non-custom ModRM helper is an observer/block boundary even without
    // block_boundary set in the catalogue (e.g. BOUND).
    let bound = d(&[0x62, 0x00], true);
    assert_eq!(bound.flow, Flow::Boundary);
    // The baseline dispatch reads ModRM before selecting the invalid unprefixed
    // SSE3 form, then delivers UD without consuming an EA displacement.
    let bare = d(&[0x0F, 0x7C, 0x05], true);
    assert_eq!(bare.length, 3);
    assert!(bare.ea.is_none());
    assert!(matches!(decode(&[0x0F, 0x7C], GuestEip(0), LinearAddress(0), true), Err(DecodeStop::Incomplete { .. })));
    let prefixed = d(&[0x66, 0x0F, 0x7C, 0x05, 1, 2, 3, 4], true);
    assert_eq!(prefixed.length, 8);
    assert!(prefixed.ea.is_some());
}
