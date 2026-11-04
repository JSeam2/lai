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

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an Apple Silicon Mac and LAI_TEST_MODEL pointing to a Gemma 4 checkpoint"]
fn gemma4_cached_decode_matches_full_recomputation() -> anyhow::Result<()> {
    use safemlx::{Array, Device, DeviceType, ExecutionContext};
    use safemlx_lm::models::{
        LoadedModel, Model,
        common::CausalLm,
        gemma4::Cache,
        input::{InputPart, ModelInput},
    };

    let path = std::env::var_os("LAI_TEST_MODEL")
        .ok_or_else(|| anyhow::anyhow!("set LAI_TEST_MODEL to a Gemma 4 model directory"))?;
    let context = ExecutionContext::new(Device::new(DeviceType::Gpu, 0));
    let weights = ExecutionContext::new(Device::new(DeviceType::Cpu, 0));
    let stream = context.stream();
    let mut loaded = LoadedModel::load(path, stream, weights.stream())?;
    let short_prompt = loaded.encode("The cat sat on the", true)?;
    let Model::Gemma4(model) = loaded.model_mut() else {
        anyhow::bail!("LAI_TEST_MODEL must be a Gemma 4 checkpoint");
    };
    let mut long_prompt = short_prompt.clone();
    long_prompt.resize(511, *short_prompt.last().unwrap());
    for mut ids in [short_prompt, long_prompt] {
        let prompt = Array::from_slice(&ids, &[1, ids.len() as i32]);
        let parts = [InputPart::text_token_ids(&prompt)];
        let mut cache = Cache::default();
        model
            .prefill_input_logits(ModelInput::new(&parts), &mut cache, stream)?
            .into_evaluated()?;

        for token in [42u32, 73, 101, 35] {
            ids.push(token);
            let next = Array::from_slice(&[token], &[1, 1]);
            let actual = model.decode_logits(&next, &mut cache, stream)?;
            let _ = actual.clone().into_evaluated()?;
            let full = Array::from_slice(&ids, &[1, ids.len() as i32]);
            let parts = [InputPart::text_token_ids(&full)];
            let mut reference_cache = Cache::default();
            let reference = model.prefill_input_logits(
                ModelInput::new(&parts),
                &mut reference_cache,
                stream,
            )?;
            let max_error = actual
                .as_type::<f32>(stream)?
                .subtract(reference.as_type::<f32>(stream)?, stream)?
                .abs(stream)?
                .max(false, stream)?
                .item::<f32>(stream);
            anyhow::ensure!(
                max_error < 0.15,
                "cached logits differ from full recomputation at {} tokens: {max_error}",
                ids.len()
            );
        }
    }
    Ok(())
}
