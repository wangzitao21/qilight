use qilight::{
    config::Device,
    spec::{Capabilities, Property},
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct SliderRanges {
    pub brightness: Vec<u32>,
    pub temperature: Vec<u32>,
}

#[derive(Clone, PartialEq, Eq)]
struct RangeKey {
    paired: bool,
    caps: Capabilities,
}
impl From<&Device> for RangeKey {
    fn from(device: &Device) -> Self {
        Self {
            paired: device.paired(),
            caps: device.caps(),
        }
    }
}

#[derive(Default)]
pub(super) struct RangeCache {
    keys: Vec<RangeKey>,
    values: Arc<SliderRanges>,
}
impl RangeCache {
    pub fn get<'a>(
        &mut self,
        devices: impl Iterator<Item = &'a Device> + Clone,
    ) -> Arc<SliderRanges> {
        // Compare only pairing and capabilities. Ordinary frames, renames and
        // changes to a light's state reuse the same arrays without copying tokens.
        if devices
            .clone()
            .map(RangeKey::from)
            .eq(self.keys.iter().cloned())
        {
            return self.values.clone();
        }
        self.keys = devices.map(RangeKey::from).collect();
        self.values = Arc::new(if self.keys.iter().all(|key| key.paired) {
            SliderRanges {
                brightness: common_values(self.keys.iter().map(|key| key.caps.brightness.clone())),
                temperature: common_values(
                    self.keys.iter().map(|key| key.caps.temperature.clone()),
                ),
            }
        } else {
            SliderRanges::default()
        });
        self.values.clone()
    }
}

fn common_values(properties: impl Iterator<Item = Option<Property>>) -> Vec<u32> {
    let Some(properties) = properties.collect::<Option<Vec<_>>>() else {
        return Vec::new();
    };
    let Some([min, max, step]) = properties.first().and_then(|p| p.range) else {
        return Vec::new();
    };
    if step == 0 || min > max || max > 100_000 {
        return Vec::new();
    }
    (min..=max)
        .step_by(step as usize)
        .filter(|n| properties.iter().all(|p| p.accepts(*n)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::demo::demo_devices;

    #[test]
    fn reuses_ranges_and_invalidates_pairing_capabilities_and_selection() {
        let mut devices = demo_devices();
        let mut cache = RangeCache::default();
        let original = cache.get(devices.iter());
        devices[0].name = "Renamed".into();
        assert!(Arc::ptr_eq(&original, &cache.get(devices.iter())));
        devices[1].token.clear();
        let unpaired = cache.get(devices.iter());
        assert!(unpaired.brightness.is_empty());
        assert!(unpaired.temperature.is_empty());
        devices[1].token = "12".repeat(16);
        assert_eq!(cache.get(devices.iter()).temperature, original.temperature);
        devices[1].model = "test".into();
        devices[1].capabilities = Some(Capabilities {
            temperature: Some(Property {
                siid: 2,
                piid: 3,
                range: Some([3000, 5000, 100]),
            }),
            ..Default::default()
        });
        let restricted = cache.get(devices.iter());
        assert_eq!(
            restricted.temperature,
            (3000..=5000).step_by(100).collect::<Vec<_>>()
        );
        assert!(restricted.brightness.is_empty());
        let single = cache.get(devices[..1].iter());
        assert_eq!(single.temperature, original.temperature);
        assert_eq!(single.brightness, original.brightness);
        let empty = cache.get(devices[..0].iter());
        assert!(empty.temperature.is_empty());
        assert!(empty.brightness.is_empty());
    }

    #[test]
    fn malformed_and_disjoint_ranges_are_disabled() {
        let property = |range| {
            Some(Property {
                siid: 2,
                piid: 3,
                range: Some(range),
            })
        };
        for range in [[0, 100, 0], [100, 0, 1], [0, u32::MAX, 1]] {
            assert!(common_values([property(range)].into_iter()).is_empty());
        }
        assert!(common_values(
            [property([3000, 4000, 100]), property([5000, 6000, 100])].into_iter()
        )
        .is_empty());
    }

    // Run explicitly in release mode; elapsed time is reported, never asserted.
    #[test]
    #[ignore]
    fn benchmark_cached_slider_ranges() {
        use std::{hint::black_box, time::Instant};
        let devices = demo_devices();
        let mut cache = RangeCache::default();
        cache.get(devices.iter());
        let iterations = 20_000;
        let start = Instant::now();
        for _ in 0..iterations {
            black_box(common_values(
                black_box(&devices).iter().map(|d| d.caps().brightness),
            ));
            black_box(common_values(
                black_box(&devices).iter().map(|d| d.caps().temperature),
            ));
        }
        let uncached = start.elapsed();
        let start = Instant::now();
        for _ in 0..iterations {
            black_box(cache.get(black_box(&devices).iter()));
        }
        let cached = start.elapsed();
        println!("{iterations} range lookups: uncached={uncached:?}, cached={cached:?}");
    }
}
