#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use regex::Regex;

#[test]
fn every_frontend_invoke_has_a_registered_handler() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib = fs::read_to_string(root.join("src/lib.rs")).expect("read lib.rs");
    let api = fs::read_to_string(root.join("../src/api.ts")).expect("read api.ts");

    let handler_start = lib.find("generate_handler![").expect("handler block");
    let handler_end = lib[handler_start..]
        .find("]);")
        .map(|offset| handler_start + offset)
        .expect("handler block end");
    let handler_block = &lib[handler_start..handler_end];
    let registered: BTreeSet<String> = Regex::new(r"commands::([a-z_]+)")
        .expect("handler regex")
        .captures_iter(handler_block)
        .map(|capture| capture[1].to_owned())
        .collect();

    let invoked: BTreeSet<String> = Regex::new(r#"call(?:<[^>]+>)?\("([a-z_]+)""#)
        .expect("invoke regex")
        .captures_iter(&api)
        .map(|capture| capture[1].to_owned())
        .collect();

    assert!(!registered.is_empty(), "no handlers found in lib.rs");
    assert!(!invoked.is_empty(), "no command calls found in api.ts");
    let missing: Vec<&String> = invoked.difference(&registered).collect();
    assert!(
        missing.is_empty(),
        "frontend calls commands without a handler: {missing:?}"
    );
    let unused: Vec<&String> = registered.difference(&invoked).collect();
    assert!(
        unused.is_empty(),
        "registered commands are not exposed in api.ts: {unused:?}"
    );
}
