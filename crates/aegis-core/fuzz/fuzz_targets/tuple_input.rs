#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = aegis_core::types::SubjectId::new(s);
        let _ = aegis_core::types::Relation::new(s);
        let _ = aegis_core::types::ResourceId::new(s);
    }
});
