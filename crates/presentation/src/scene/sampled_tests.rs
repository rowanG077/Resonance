use super::*;
#[test]
fn bindings_share_images_but_distinct_samplers_and_retired_fields_do_not() {
    let mut images = Assets::<Image>::default();
    let source = images.add(Image::default());
    let mut cache = SampledImages::default();
    let mut binding = TextureBinding {
        texture: 0,
        wrap_u: TextureWrap::Clamp,
        wrap_v: TextureWrap::Clamp,
        nearest_min: false,
        nearest_mag: false,
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
    assert_eq!((cache.hits, cache.misses), (1, 3));
}
