//! Opt-in integration test for a real downloaded MLX model.

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an Apple Silicon Mac and LAI_TEST_MODEL pointing to a downloaded model"]
fn loads_and_generates_from_configured_mlx_checkpoint() -> anyhow::Result<()> {
    use safemlx::{Device, DeviceType, ExecutionContext};
    use safemlx_lm::models::{
        LoadedModel,
        input::{InputPart, ModelInput},
    };

    let path = std::env::var_os("LAI_TEST_MODEL")
        .ok_or_else(|| anyhow::anyhow!("set LAI_TEST_MODEL to the downloaded model directory"))?;
    let context = ExecutionContext::new(Device::new(DeviceType::Gpu, 0));
    let weights_context = ExecutionContext::new(Device::new(DeviceType::Cpu, 0));
    let stream = context.stream();
    let mut model = LoadedModel::load(path, stream, weights_context.stream())?;
    let tokens = model.encode_to_array("Hello", true, stream)?;
    let mut cache = model.new_cache();
    let parts = [InputPart::text_token_ids(&tokens)];
    let mut generator =
        model.generate_input_with_cache(&mut cache, 0.0, ModelInput::new(&parts), None, stream);
    let token = generator
        .next()
        .ok_or_else(|| anyhow::anyhow!("model generated no token"))??;
    let _ = token.item::<u32>(stream);
    Ok(())
}
