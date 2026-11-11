use super::*;
use bevy::image::{ImageFilterMode, ImageSampler};

#[test]
fn encounter_retention_keeps_only_selected_sources_and_reuses_uploaded_samplers() {
    let mut images = Assets::<Image>::default();
    let a = images.add(Image::default());
    let b = images.add(Image::default());
    let binding = TextureBinding {
        texture: 0,
        wrap_u: TextureWrap::Clamp,
        wrap_v: TextureWrap::Clamp,
        nearest_min: false,
        nearest_mag: false,
        mipmapped: false,
        scale: TextureBinding::unscaled(),
    };
    let mut cache = SampledImages::default();
    let mut next_encounter = cache.clone();
    let sampled_a =
        sampled_image(Some((a.clone(), binding.clone())), &mut images, &mut cache).unwrap();
    let sampled_b =
        sampled_image(Some((b.clone(), binding.clone())), &mut images, &mut cache).unwrap();
    assert_eq!(next_encounter.stats(), (0, 2));
    let old_a = sampled_a.id();
    let old_b = sampled_b.id();
    let retained = cache.retained_for(std::iter::once(a.id()));
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].id(), old_a);
    drop(sampled_a);
    drop(sampled_b);
    next_encounter.prune();
    let next_a =
        sampled_image(Some((a, binding.clone())), &mut images, &mut next_encounter).unwrap();
    let next_b = sampled_image(
        Some((b.clone(), binding.clone())),
        &mut images,
        &mut next_encounter,
    )
    .unwrap();
    assert_eq!(next_a.id(), old_a);
    assert_ne!(next_b.id(), old_b);
    assert_eq!(
        sampled_image(Some((b, binding)), &mut images, &mut cache).unwrap(),
        next_b,
        "samplers created through a clone must be shared with the original owner"
    );
    assert_eq!(cache.stats(), (2, 3));
    assert_eq!(next_encounter.stats(), cache.stats());
}

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
        scale: TextureBinding::unscaled(),
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
    assert!(
        derived.data.is_none(),
        "sampler variants never duplicate pixels"
    );
    assert!(images.get(&source).unwrap().data.is_some());
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
    assert!(
        derived.data.is_none(),
        "sampler variants never duplicate pixels"
    );
    assert!(images.get(&source).unwrap().data.is_some());
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
    assert_eq!(cache.stats(), (1, 4));
}
