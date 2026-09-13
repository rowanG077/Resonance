//! Mesa reads these options at process initialization. Use exec instead of
//! mutating the environment after Bevy or a driver has started worker threads.

/// Apply the aarch64 Linux Solari prototype's driver workarounds.
///
/// Call from an executable's entry point, before creating output files or an
/// App. May replace the process once, preserving its arguments and exit status.
/// Other platforms and builds without Solari are unaffected.
pub fn prepare_ray_tracing_process() -> anyhow::Result<()> {
    #[cfg(all(feature = "solari", target_os = "linux", target_arch = "aarch64"))]
    {
        use std::{env, os::unix::process::CommandExt, process::Command};

        // Mesa 26.2's ray-geometry builder needs eight lanes. Reusing the
        // user's disk shader cache also reproduced a raster JIT segfault with
        // 256-bit vectors. Bypass that cache without deleting any user data.
        // Apply before adapter selection so implicit software fallback is safe
        // too. The cache bypass also covers GPU rendering in this prototype.
        let settings = [
            ("LP_NATIVE_VECTOR_WIDTH", "256"),
            ("MESA_SHADER_CACHE_DISABLE", "true"),
        ];
        if settings
            .iter()
            .any(|(key, value)| env::var_os(key).as_deref() != Some(value.as_ref()))
        {
            eprintln!(
                "Resonance: applying aarch64 Solari driver settings (256-bit software vectors; Mesa disk shader cache disabled)"
            );
            let mut command = Command::new(env::current_exe()?);
            command.args(env::args_os().skip(1)).envs(settings);
            if env::var_os("LP_NUM_THREADS").is_none() {
                command.env("LP_NUM_THREADS", "4");
            }
            return Err(command.exec().into());
        }
    }
    Ok(())
}
