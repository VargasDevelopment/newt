// MIT. Shared legacy host fixture resources; all three callers lend the same fields.
canonical::Resources {
    policy: None,
    scan: &mut scan,
    sequential: &mut sequential,
    entries: &mut entries,
    manifest: &mut manifest,
    spine_hashes: &mut hashes,
    spine: &mut spines,
    backend: &mut backend,
    decoder: Some(InflateScratch {
        state: &mut state,
        dictionary: dict.as_mut_slice().try_into().unwrap(),
        input: input.as_mut_slice().try_into().unwrap(),
    }),
    chunk: Some(&mut chunk),
    xml_buffer: Some(&mut xml),
    filter: Some(&mut filter),
}
