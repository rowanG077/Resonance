//! Native title artwork on the shared GameCube UI canvas.
use crate::{output, package::Package, source::Source, ui_images};
use anyhow::{Context, Result, ensure};
use resonance_content::startup::{
    self,
    boot::{Logo, Sequence},
};
use std::path::Path;

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    let names = [
        "continue-alternate",
        "continue-alternate-selected",
        "logo",
        "copyright",
        "new-game-highlighted",
        "new-game",
        "continue-highlighted",
        "continue",
        "customize-highlighted",
        "customize",
        "exit-game-highlighted",
        "exit-game",
        "sound-test-highlighted",
        "sound-test",
        "tower-background",
        "lloyd-in-a-field",
    ];
    let bytes = source.require("TITLE.PAC")?;
    let package = Package::parse(&bytes)?;
    ensure!(
        package.len() == names.len(),
        "unexpected title image directory"
    );
    let mut sizes = std::collections::BTreeMap::new();
    for (index, name) in names.iter().enumerate() {
        let image = ui_images::cook(
            source,
            output,
            package.reference(index)?.context("missing title image")?,
            &format!("ui/title/{name}.ktx2"),
            index != 15,
        )?;
        sizes.insert(*name, [image.width, image.height]);
    }
    let mut logos = Vec::new();
    for (name, path, timing) in [
        ("BOOT_PIC000.TOTEXB_D", "controller-notice", None),
        (
            "BOOT_PIC001.TOTEXB_D",
            "bandai-namco",
            Some((240, Some(120))),
        ),
        ("BOOT_PIC002.TOTEXB_D", "namco", Some((180, None))),
        ("CRIWARE_LOGO.TOTEXB_D", "criware", Some((120, None))),
    ] {
        let image = ui_images::cook(source, output, name, &format!("ui/boot/{path}.ktx2"), true)?;
        if let Some((fade_out_at, skip_after)) = timing {
            logos.push(Logo {
                image: image.path,
                fade_out_at,
                skip_after,
                fade_ticks: 64,
                background: [255; 3],
            });
        }
    }
    let boot = Sequence(logos);
    boot.duration()?;
    output::json(&output.join(startup::BOOT), &boot)?;
    let title = resonance_importer::title_definition(&sizes);
    title.validate()?;
    output::json(&output.join(startup::PATH), &title)
}
