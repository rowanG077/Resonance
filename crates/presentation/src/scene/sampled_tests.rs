use super::*;
#[test]
fn bindings_share_images_but_distinct_samplers_and_retired_fields_do_not() {
    let mut images = Assets::<Image>::default();
    let mut original = Image::default();
    original.texture_descriptor.size.width = 2;
    original.texture_descriptor.size.height = 2;
    original.texture_descriptor.mip_level_count = 2;
    original.data = Some(vec![255; 20]);
    let source = images.add(original);
    let mut cache = SampledImages::default();
    let mut binding = TextureBinding {
        texture: 0,
        wrap_u: TextureWrap::Clamp,
        wrap_v: TextureWrap::Clamp,
        nearest_min: false,
        nearest_mag: false,
        mipmapped: false,
    };
    let first = sampled_image(
        Some((source.clone(), binding.clone())),
        &mut images,
        &mut cache,
    )
    .unwrap();
    let next = sampled_image(
        Some((source.clone(), binding.clone())),
        &mut images,
        &mut cache,
    )
    .unwrap();
    assert_eq!(first.id(), next.id());
    let derived = images.get(&first).unwrap();
    assert_eq!(derived.texture_descriptor.mip_level_count, 2);
    let view = derived.texture_view_descriptor.as_ref().unwrap();
    assert_eq!((view.base_mip_level, view.mip_level_count), (0, Some(1)));
    assert_eq!(derived.data, images.get(&source).unwrap().data);
    assert!(
        images
            .get(&source)
            .unwrap()
            .texture_view_descriptor
            .is_none()
    );
    binding.mipmapped = true;
    let mipped = sampled_image(
        Some((source.clone(), binding.clone())),
        &mut images,
        &mut cache,
    )
    .unwrap();
    assert_ne!(first.id(), mipped.id());
    let derived = images.get(&mipped).unwrap();
    assert_eq!(
        derived
            .texture_view_descriptor
            .as_ref()
            .unwrap()
            .mip_level_count,
        None
    );
    let ImageSampler::Descriptor(sampler) = &derived.sampler else {
        panic!("material sampler must be explicit");
    };
    assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
    assert_eq!(derived.texture_descriptor.mip_level_count, 2);
    assert_eq!(derived.data, images.get(&source).unwrap().data);
    binding.wrap_u = TextureWrap::Repeat;
    let distinct = sampled_image(
        Some((source.clone(), binding.clone())),
        &mut images,
        &mut cache,
    )
    .unwrap();
    assert_ne!(first.id(), distinct.id());
    let retired = distinct.id();
    drop(distinct);
    let fresh = sampled_image(Some((source, binding)), &mut images, &mut cache).unwrap();
    assert_ne!(
        retired,
        fresh.id(),
        "weak cache must release retired bindings"
    );
    assert_eq!((cache.hits, cache.misses), (1, 4));
}
