use super::*;
use crate::library::{Library, Mod};
use sha2::Digest;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let fixture = Self(std::env::temp_dir().join(format!(
            "resonance-save-gameplay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&fixture.0)?;
        fixture.write(
            "base/content.json",
            br#"{"version":1,"game":"GQSEAF","revision":0}"#,
        )?;
        Ok(fixture)
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap())?;
        Ok(fs::write(path, bytes)?)
    }
    fn base(&self) -> PathBuf {
        self.0.join("base")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn contact_model() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "model": {
            "rig": {"skeleton": {"bones": [{"name":"root", "parent":null, "bind_channels":7,
                "bind":{"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.],"scale":[1.,1.,1.]}},
                {"name":"tip", "parent":0, "bind_channels":7,
                "bind":{"translation":[0.,1.,0.],"rotation":[0.,0.,0.,1.],"scale":[1.,1.,1.]}}]},
                "attachments":{"0":0}},
            "layers": [{"animation":0, "attached_to":null, "additive":false,
                "scene":{"resource":0, "mesh":"missing.glb", "textures":["missing.png"],
                    "materials":[], "translation":[0.,0.,0.], "autoplay":false,
                    "bone_names":["root","tip"], "texture_animations":[],
                    "clips":[{"motion":"swing.motion.json","resource_slot":0}]}}]
        }
    })
}

fn model_identity(snapshot: &mut Snapshot, path: &str) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    // Exercise the ordinary equipment lookup instead of hashing a filename.
    let weapon = crate::battle_model::document::WeaponDefinition {
        parts: [(0, path.to_owned())].into(),
        trails: Default::default(),
        uv_animation: Default::default(),
    };
    models::weapon(snapshot, &mut hash, "item/7", weapon, &|| false)?;
    Ok(hash.finalize().into())
}

#[test]
fn contact_identity_follows_current_rigs_and_motions_without_mesh_or_texture_payloads() -> Result<()>
{
    let fixture = Fixture::new()?;
    let motion = crate::animation::Motion {
        duration_frames: 12.,
        tracks: vec![],
    };
    fixture.write("base/swing.motion.json", &motion.encode()?)?;
    let original = contact_model();
    fixture.write("base/model.json", &serde_json::to_vec(&original)?)?;
    let library = Library::open(&fixture.base(), &[])?;
    let mut retained = library.snapshot();
    let baseline = model_identity(&mut retained, "model.json")?;

    let mut edited = original.clone();
    edited["model"]["layers"][0]["scene"]["mesh"] = "repainted.glb".into();
    edited["model"]["layers"][0]["scene"]["textures"] = serde_json::json!(["repainted.png"]);
    edited["model"]["layers"][0]["additive"] = true.into();
    edited["model"]["layers"][0]["scene"]["translation"] = serde_json::json!([10., 20., 30.]);
    fixture.write("base/model.json", &serde_json::to_vec_pretty(&edited)?)?;
    assert_eq!(
        model_identity(&mut library.snapshot(), "model.json")?,
        baseline
    );

    let mut secondary = edited.clone();
    secondary["model"]["layers"][0]["scene"]["secondary_motion"] = serde_json::json!({
        "chains": [{"joints": [{"node":0,"gravity":0.1,"damping":0.5},
                {"node":1,"gravity":0.1,"damping":0.5}],
            "attraction":0.1,"preserve_rotation":false,"rotation_locks":[false,false],
            "collision_plane":null}]
    });
    fixture.write("base/secondary.json", &serde_json::to_vec(&secondary)?)?;
    assert_ne!(
        model_identity(&mut library.snapshot(), "secondary.json")?,
        baseline
    );

    fixture.write("base/renamed/motion.json", &motion.encode()?)?;
    edited["model"]["layers"][0]["scene"]["clips"][0]["motion"] = "renamed/motion.json".into();
    fixture.write(
        "base/renamed/model.json",
        &serde_json::to_vec_pretty(&edited)?,
    )?;
    assert_eq!(
        model_identity(&mut library.snapshot(), "renamed/model.json")?,
        baseline
    );

    edited["model"]["rig"]["skeleton"]["bones"][0]["bind"]["translation"][0] = 4.into();
    fixture.write("mod/renamed/model.json", &serde_json::to_vec(&edited)?)?;
    let modified = Library::open(
        &fixture.base(),
        &[Mod {
            name: "reach".into(),
            root: fixture.0.join("mod"),
        }],
    )?;
    assert_ne!(
        model_identity(&mut modified.snapshot(), "renamed/model.json")?,
        baseline
    );
    assert_eq!(
        model_identity(&mut library.snapshot(), "renamed/model.json")?,
        baseline
    );

    fixture.write(
        "base/swing.motion.json",
        &crate::animation::Motion {
            duration_frames: 24.,
            tracks: vec![],
        }
        .encode()?,
    )?;
    assert_ne!(
        model_identity(&mut library.snapshot(), "model.json")?,
        baseline
    );
    assert_eq!(model_identity(&mut retained, "model.json")?, baseline);
    assert!(!fixture.base().join("missing.glb").exists());
    Ok(())
}

#[test]
fn action_projection_preserves_movement_and_ignores_audio_and_camera_choices() -> Result<()> {
    let mut first = vec![
        (0, Event::Forward(3.)),
        (1, Event::Sound(7)),
        (2, Event::Voice(None)),
    ];
    let mut second = vec![
        (0, Event::Forward(3.)),
        (10, Event::Sound(11)),
        (
            2,
            Event::CameraDistance {
                distance: 100.,
                duration: 5,
            },
        ),
    ];
    normal_events(&mut first);
    normal_events(&mut second);
    assert_eq!(serde_json::to_vec(&first)?, serde_json::to_vec(&second)?);
    second[0].1 = Event::Forward(4.);
    assert_ne!(serde_json::to_vec(&first)?, serde_json::to_vec(&second)?);
    Ok(())
}
