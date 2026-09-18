//! Export the current shared encoder policy for isolated benchmark adapters.
fn main() -> anyhow::Result<()> {
    let args: Vec<u32> = std::env::args()
        .skip(1)
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?;
    anyhow::ensure!(
        args.len() == 3,
        "usage: encoder-options FPS BITRATE_KBPS QUALITY"
    );
    let profiles: serde_json::Map<_, _> = uscreen_config::encoding::ENCODERS
        .iter()
        .map(|encoder| {
            let profile =
                uscreen_config::encoding::Profile::new(encoder.name, args[0], args[1], args[2])
                    .unwrap();
            (
                encoder.name.to_string(),
                serde_json::json!({
                    "encoder": uscreen_config::encoding::ffmpeg_name(encoder.name),
                    "options": profile.cli_options(false),
                }),
            )
        })
        .collect();
    println!("{}", serde_json::to_string(&profiles)?);
    Ok(())
}
