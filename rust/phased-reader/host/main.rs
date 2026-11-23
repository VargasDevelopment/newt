//! Parent maps this existing source entry to its approved host package; no new
//! manifest or dependency acquisition is supplied by this packet.
mod entry;
mod language;
mod physical;
mod run_budget;
fn main() -> std::io::Result<()> {
    use entry::Case::*;
    let args: Vec<_> = std::env::args_os().collect();
    if let [_, flag, input, ..] = args.as_slice() {
        let path = std::path::Path::new(input);
        if flag == "--language" {
            return language::run(path);
        }
        if flag == "--run-budget" {
            return run_budget::run(path);
        }
    }
    if args
        .get(1)
        .is_some_and(|a| a == "--oracle" || a == "--menu-oracle")
    {
        entry::run(
            std::path::Path::new(args.get(2).expect("EPUB path")),
            if args[1] == "--menu-oracle" {
                MenuOracle {
                    anchors: args.iter().any(|a| a == "--anchors"),
                }
            } else {
                Oracle
            },
            args.iter().any(|a| a == "--landscape"),
        );
        return Ok(());
    }
    let root = std::path::PathBuf::from(std::env::args_os().nth(1).expect("fixture directory"));
    for (name, case) in [
        ("menu", Menu),
        ("plain", Plain),
        ("cross-spine", CrossSpine),
        ("inline", Plain),
        ("linked", Plain),
        ("late-malformed", LateMalformed),
        ("deferred", Deferred),
        ("pending", Pending),
        ("optional-malformed", OptionalMalformed),
        ("optional-live-fault", OptionalLiveFault),
        ("duplicate-id", Duplicate),
        ("collision", Collision),
        ("prefix-anchor", PrefixAnchor),
    ] {
        entry::run(&root.join(format!("{name}.epub")), case, false);
        println!("{name}: operation assertions passed");
    }
    Ok(())
}
