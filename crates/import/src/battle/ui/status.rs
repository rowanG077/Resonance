//! Status artwork shares the indexed battle HUD atlas.
use anyhow::Result;
use resonance_content::{
    battle::ui::{StatusArt, StatusBadge, StatusBadgeArt},
    font::UiRegion,
};
use std::collections::BTreeMap;

pub(super) fn cook(
    sprite: &mut impl FnMut(usize, [u32; 4]) -> Result<UiRegion>,
) -> Result<StatusArt> {
    Ok(StatusArt {
        curse: sprite(42, [0, 72, 24, 24])?,
        curse_cross: sprite(42, [256, 80, 24, 24])?,
        // The 32-pixel window moves through all 32 horizontal phases.
        hp_shimmer: sprite(0, [448, 0, 64, 32])?,
        // Magical item recovery penalty, then its bouncing arrow.
        item_recovery_down: Some(sprite(43, [32, 72, 24, 24])?),
        down_arrow: Some(sprite(92, [328, 56, 24, 24])?),
        up_arrow: Some(sprite(91, [304, 56, 24, 24])?),
        attack: Some(sprite(94, [288, 136, 24, 24])?),
        defense: Some(sprite(93, [352, 56, 24, 24])?),
        magic: Some(sprite(87, [328, 80, 24, 24])?),
        physical_ailment_immunity: Some(sprite(89, [256, 56, 24, 24])?),
        magical_ailment_immunity: Some(sprite(90, [280, 56, 24, 24])?),
        badges: bind_badges(sprite)?,
    })
}

fn bind_badges(
    sprite: &mut impl FnMut(usize, [u32; 4]) -> Result<UiRegion>,
) -> Result<BTreeMap<StatusBadge, StatusBadgeArt>> {
    [
        (StatusBadge::AnimatedCondition, 44, [64, 72, 24, 24]),
        (StatusBadge::HolySong, 88, [352, 80, 24, 24]),
    ]
    .into_iter()
    .map(|(badge, palette, rect)| {
        let frames = (palette..palette + badge.frames())
            .map(|palette| sprite(palette, rect))
            .collect::<Result<_>>()?;
        Ok((badge, StatusBadgeArt { frames }))
    })
    .collect()
}

#[test]
#[ignore = "requires both original discs and all-assets; reads shared HUD artwork without conversion"]
fn original_status_badges_bind_shared_palettes_and_holy_song_dispatch() -> Result<()> {
    use crate::{
        battle::{actions::Rel, all::Sources, visual::binding::Directory},
        read::u16 as half,
    };
    use std::{fs, path::Path};
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = project.join("local/all-assets");
    let mut shared_sources = BTreeMap::new();
    for disc in 1..=2 {
        let extracted = project.join(format!("local/extracted/disc{disc}"));
        let sources = Sources::read(&extracted)?;
        let directories = Directory::open(&output, disc, &sources.usual)?;
        let (directory, _) = directories.resolve("battle/usual/4/3/textures.json")?;
        let source_hash = crate::digest(&fs::read(extracted.join("files").join(&sources.usual))?);
        if let Some(previous) = shared_sources.insert(directory.to_owned(), source_hash.clone()) {
            assert_eq!(
                previous, source_hash,
                "shared HUD directories require identical source bytes"
            );
        }
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
        assert_eq!(
            crate::digest(&rel.at((1, 0x6bd54))?[..0x1098]),
            "27eff02c7ccbeedaa10b42a2bf50f83759b0ed3ffa840c418c2783c6b98925a3"
        );
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let native = half(crate::dol::slice(&executable, 0x80202f90 + 57 * 88, 88)?, 0)?;
        assert_eq!(native, 268);
        let dispatch = rel.pointer(5, 0x1238 + usize::from(native - 200) * 4)?;
        assert_eq!(rel.pointer(dispatch.0, dispatch.1)?, (1, 0x7ad90));
        let atlas = super::hud(&output, disc, &sources)?;
        let art = bind_badges(&mut |palette, rect| atlas.region(palette, rect))?;
        for (badge, palette, rect) in [
            (StatusBadge::AnimatedCondition, 44, [64, 72, 24, 24]),
            (StatusBadge::HolySong, 88, [352, 80, 24, 24]),
        ] {
            assert_eq!(art[&badge].frames.len(), badge.frames());
            for (frame, texture) in art[&badge].frames.iter().enumerate() {
                assert_eq!(texture.rect, rect);
                assert_eq!(
                    texture.texture.path,
                    format!(
                        "{directory}/battle/usual/4/3/texture-0-palette-{}.ktx2",
                        palette + frame
                    )
                );
                assert_eq!([texture.texture.width, texture.texture.height], [512, 512]);
                assert_eq!(
                    &fs::read(output.join(&texture.texture.path))?[..12],
                    b"\xabKTX 20\xbb\r\n\x1a\n"
                );
            }
        }
    }
    Ok(())
}
