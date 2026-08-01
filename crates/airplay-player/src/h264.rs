#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NalKinds {
    pub has_sps: bool,
    pub has_pps: bool,
    pub has_idr: bool,
    pub has_vcl: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum H264InputError {
    #[error("H.264 buffer does not begin with an Annex-B start code")]
    MissingStartCode,
    #[error("Annex-B start code is not followed by a NAL unit")]
    EmptyNal,
    #[error("H.264 buffer contains no video or codec configuration NAL")]
    NoVideoData,
}

fn start_code_at(data: &[u8], offset: usize) -> Option<usize> {
    if data.get(offset..offset + 4) == Some(&[0, 0, 0, 1]) {
        Some(4)
    } else if data.get(offset..offset + 3) == Some(&[0, 0, 1]) {
        Some(3)
    } else {
        None
    }
}

fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    (from..data.len()).find_map(|offset| start_code_at(data, offset).map(|len| (offset, len)))
}

fn nal_units(data: &[u8]) -> Result<Vec<&[u8]>, H264InputError> {
    let Some(first_len) = start_code_at(data, 0) else {
        return Err(H264InputError::MissingStartCode);
    };

    let mut units = Vec::new();
    let mut nal_start = first_len;
    loop {
        let next = find_start_code(data, nal_start);
        let nal_end = next.map(|(offset, _)| offset).unwrap_or(data.len());
        if nal_start == nal_end {
            return Err(H264InputError::EmptyNal);
        }
        units.push(&data[nal_start..nal_end]);
        let Some((offset, prefix_len)) = next else {
            break;
        };
        nal_start = offset + prefix_len;
        if nal_start == data.len() {
            return Err(H264InputError::EmptyNal);
        }
    }
    Ok(units)
}

pub fn classify_annex_b(data: &[u8]) -> Result<NalKinds, H264InputError> {
    let mut kinds = NalKinds::default();
    for nal in nal_units(data)? {
        let nal_type = nal[0] & 0x1f;
        match nal_type {
            7 => kinds.has_sps = true,
            8 => kinds.has_pps = true,
            5 => {
                kinds.has_idr = true;
                kinds.has_vcl = true;
            }
            1..=4 => kinds.has_vcl = true,
            _ => {}
        }
    }
    Ok(kinds)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateResult {
    CodecConfigUpdated,
    WaitingForConfig,
    WaitingForIdr,
    AccessUnit(Vec<u8>),
    Rejected(H264InputError),
}

#[derive(Debug, Default)]
pub struct CodecGate {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    idr_ready: bool,
}

impl CodecGate {
    pub fn push(&mut self, data: &[u8]) -> GateResult {
        let units = match nal_units(data) {
            Ok(units) => units,
            Err(error) => return GateResult::Rejected(error),
        };

        let mut kinds = NalKinds::default();
        let mut config_changed = false;
        for nal in &units {
            let nal_type = nal[0] & 0x1f;
            match nal_type {
                7 => {
                    kinds.has_sps = true;
                    if self.sps.as_deref() != Some(*nal) {
                        self.sps = Some((*nal).to_vec());
                        config_changed = true;
                    }
                }
                8 => {
                    kinds.has_pps = true;
                    if self.pps.as_deref() != Some(*nal) {
                        self.pps = Some((*nal).to_vec());
                        config_changed = true;
                    }
                }
                5 => {
                    kinds.has_idr = true;
                    kinds.has_vcl = true;
                }
                1..=4 => kinds.has_vcl = true,
                _ => {}
            }
        }

        if config_changed {
            self.idr_ready = false;
        }
        if !kinds.has_vcl {
            return if kinds.has_sps || kinds.has_pps {
                GateResult::CodecConfigUpdated
            } else {
                GateResult::Rejected(H264InputError::NoVideoData)
            };
        }
        let (Some(sps), Some(pps)) = (&self.sps, &self.pps) else {
            return GateResult::WaitingForConfig;
        };
        if !self.idr_ready && !kinds.has_idr {
            return GateResult::WaitingForIdr;
        }

        if kinds.has_idr {
            self.idr_ready = true;
            let mut access_unit = Vec::with_capacity(8 + sps.len() + pps.len() + data.len());
            access_unit.extend_from_slice(&[0, 0, 0, 1]);
            access_unit.extend_from_slice(sps);
            access_unit.extend_from_slice(&[0, 0, 0, 1]);
            access_unit.extend_from_slice(pps);
            access_unit.extend_from_slice(data);
            GateResult::AccessUnit(access_unit)
        } else {
            GateResult::AccessUnit(data.to_vec())
        }
    }

    pub fn reset(&mut self) {
        self.sps = None;
        self.pps = None;
        self.idr_ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annex_b(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(nal);
        }
        out
    }

    fn ready_gate() -> CodecGate {
        let mut gate = CodecGate::default();
        assert_eq!(
            gate.push(&annex_b(&[&[0x67, 2], &[0x68, 3]])),
            GateResult::CodecConfigUpdated
        );
        assert!(matches!(
            gate.push(&annex_b(&[&[0x65, 5]])),
            GateResult::AccessUnit(_)
        ));
        gate
    }

    #[test]
    fn classifies_parameter_sets_and_picture_types() {
        let config = annex_b(&[&[0x67, 1], &[0x68, 2]]);
        let kinds = classify_annex_b(&config).unwrap();
        assert!(kinds.has_sps && kinds.has_pps);
        assert!(!kinds.has_vcl);

        let idr = annex_b(&[&[0x65, 3]]);
        let kinds = classify_annex_b(&idr).unwrap();
        assert!(kinds.has_idr && kinds.has_vcl);

        let inter = annex_b(&[&[0x41, 4]]);
        let kinds = classify_annex_b(&inter).unwrap();
        assert!(kinds.has_vcl && !kinds.has_idr);
    }

    #[test]
    fn accepts_three_byte_start_codes_for_inspection() {
        let kinds = classify_annex_b(&[0, 0, 1, 0x65, 1]).unwrap();
        assert!(kinds.has_idr);
    }

    #[test]
    fn rejects_missing_start_code_and_empty_nal() {
        assert!(matches!(
            classify_annex_b(&[0x65, 1]),
            Err(H264InputError::MissingStartCode)
        ));
        assert!(matches!(
            classify_annex_b(&[0, 0, 0, 1]),
            Err(H264InputError::EmptyNal)
        ));
    }

    #[test]
    fn gate_waits_for_config_then_idr_and_prefixes_config() {
        let mut gate = CodecGate::default();
        assert_eq!(
            gate.push(&annex_b(&[&[0x41, 1]])),
            GateResult::WaitingForConfig
        );
        assert_eq!(
            gate.push(&annex_b(&[&[0x67, 2], &[0x68, 3]])),
            GateResult::CodecConfigUpdated
        );
        assert_eq!(
            gate.push(&annex_b(&[&[0x41, 4]])),
            GateResult::WaitingForIdr
        );
        let GateResult::AccessUnit(bytes) = gate.push(&annex_b(&[&[0x65, 5]])) else {
            panic!("expected IDR access unit");
        };
        let kinds = classify_annex_b(&bytes).unwrap();
        assert!(kinds.has_sps && kinds.has_pps && kinds.has_idr);
    }

    #[test]
    fn new_config_and_disconnect_require_a_new_idr() {
        let mut gate = ready_gate();
        assert!(matches!(
            gate.push(&annex_b(&[&[0x41, 6]])),
            GateResult::AccessUnit(_)
        ));
        assert_eq!(
            gate.push(&annex_b(&[&[0x67, 7], &[0x68, 8]])),
            GateResult::CodecConfigUpdated
        );
        assert_eq!(
            gate.push(&annex_b(&[&[0x41, 9]])),
            GateResult::WaitingForIdr
        );
        gate.reset();
        assert_eq!(
            gate.push(&annex_b(&[&[0x65, 10]])),
            GateResult::WaitingForConfig
        );
    }

    #[test]
    fn every_idr_is_prefixed_with_cached_parameter_sets() {
        let mut gate = ready_gate();
        let GateResult::AccessUnit(bytes) = gate.push(&annex_b(&[&[0x65, 11]])) else {
            panic!("expected IDR access unit");
        };
        let kinds = classify_annex_b(&bytes).unwrap();
        assert!(kinds.has_sps && kinds.has_pps && kinds.has_idr);
    }
}
