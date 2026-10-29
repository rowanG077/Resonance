use std::fs;
use symphonia_script::{Op, Program, scenario};
use symphonia_script_tools::run;

#[test]
fn scenario_editing_round_trips_raw_data_and_publishes_changed_instructions()
-> Result<(), Box<dyn std::error::Error>> {
    let work = tempfile::tempdir()?;
    let input = work.path().join("scenario.ssb");
    let source = work.path().join("scenario.ssasm");
    let target = work.path().join("mod/fields/iselia/scripts/scenario.ssb");
    let original = scenario::assemble(
        ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\npush.s8 7\nend\n.byte 0xAB\n",
    )?;
    fs::write(&input, &original)?;
    let command = |operation: &str,
                   input: &std::path::Path,
                   output: &std::path::Path|
     -> Result<(), symphonia_script_tools::Error> {
        run(
            [
                "scenario".into(),
                operation.into(),
                input.to_string_lossy().into_owned(),
                output.to_string_lossy().into_owned(),
            ],
            &[],
            &mut Vec::new(),
        )
    };
    command("disassemble", &input, &source)?;
    let listing = fs::read_to_string(&source)?;
    assert_eq!(scenario::assemble(&listing)?, original);
    assert!(listing.contains(".byte 0xAB"));
    command("assemble", &source, &target)?;
    assert_eq!(fs::read(&target)?, original);
    let edited = listing.replace("push.s8 0x07", "push.s8 0x09");
    assert_ne!(edited, listing);
    fs::write(&source, &edited)?;
    command("assemble", &source, &target)?;
    let changed = fs::read(&target)?;
    assert_eq!(
        Program::decode(&changed)?.instruction(0),
        Some((Op::Push(9), 1))
    );
    assert_eq!(changed.last(), Some(&0xAB));
    assert_eq!(
        fs::read(&input)?,
        original,
        "writing a mod must not edit the base"
    );
    fs::write(&source, ".scenario\ninvalid instruction\n")?;
    assert!(command("assemble", &source, &target).is_err());
    assert_eq!(
        fs::read(&target)?,
        changed,
        "failed assembly must leave the prior file intact"
    );
    assert!(command("assemble", &input, &target).is_err());
    Ok(())
}
