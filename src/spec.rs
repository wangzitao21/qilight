use crate::{message, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Property {
    pub siid: u32,
    pub piid: u32,
    pub range: Option<[u32; 3]>,
}
impl Property {
    pub fn accepts(&self, value: u32) -> bool {
        self.range.is_some_and(|[min, max, step]| {
            step > 0 && value >= min && value <= max && (value - min).is_multiple_of(step)
        })
    }
    pub fn validate(&self, value: u32) -> Result<()> {
        let [min, max, step] = self
            .range
            .ok_or_else(|| message("该型号没有可验证的调节范围，请重新扫码读取规格。"))?;
        if step == 0 || value < min || value > max || !(value - min).is_multiple_of(step) {
            return Err(message(format!("数值应在 {min}–{max} 之间，步长 {step}。")));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub power: Option<Property>,
    pub brightness: Option<Property>,
    pub temperature: Option<Property>,
}

fn is_type(value: &Value, kind: &str) -> bool {
    value["type"]
        .as_str()
        .is_some_and(|s| s.split(':').nth(3) == Some(kind))
}

impl Capabilities {
    pub fn for_model(model: &str) -> Self {
        if model == "yeelink.light.lamp22" {
            // Verified main-light properties for yeelink.light.lamp22.
            // Keep these runtime defaults in source, independent of documentation.
            Self {
                power: Some(Property {
                    siid: 2,
                    piid: 1,
                    range: None,
                }),
                brightness: Some(Property {
                    siid: 2,
                    piid: 2,
                    range: Some([1, 100, 1]),
                }),
                temperature: Some(Property {
                    siid: 2,
                    piid: 3,
                    range: Some([2700, 6500, 1]),
                }),
            }
        } else {
            Self::default()
        }
    }
    pub fn from_spec(spec: &Value) -> Result<Self> {
        let services = spec["services"]
            .as_array()
            .ok_or_else(|| message("型号规格缺少服务列表。"))?;
        // Use only the main light service, never background light or scene parameters.
        let light = services
            .iter()
            .find(|s| is_type(s, "light"))
            .ok_or_else(|| message("该型号未提供灯光服务。"))?;
        let siid = light["iid"]
            .as_u64()
            .and_then(|x| u32::try_from(x).ok())
            .ok_or_else(|| message("灯光服务编号无效。"))?;
        let properties = light["properties"]
            .as_array()
            .ok_or_else(|| message("型号规格缺少属性。"))?;
        let get = |name: &str| -> Option<Property> {
            let p = properties.iter().find(|p| {
                is_type(p, name)
                    && ["read", "write"].iter().all(|a| {
                        p["access"]
                            .as_array()
                            .is_some_and(|access| access.iter().any(|v| v.as_str() == Some(a)))
                    })
            })?;
            let range = p["value-range"].as_array().and_then(|values| {
                if values.len() != 3 {
                    return None;
                }
                let mut r = [0; 3];
                for (i, v) in values.iter().enumerate() {
                    r[i] = u32::try_from(v.as_u64()?).ok()?;
                }
                (r[0] <= r[1] && r[2] > 0).then_some(r)
            });
            if name != "on" && range.is_none() {
                return None;
            }
            if name == "brightness" && (p["unit"] != "percentage" || range?[1] > 100) {
                return None;
            }
            Some(Property {
                siid,
                piid: p["iid"].as_u64()?.try_into().ok()?,
                range,
            })
        };
        let caps = Self {
            power: get("on"),
            brightness: get("brightness"),
            temperature: get("color-temperature"),
        };
        if caps.power.is_none() {
            return Err(message("该灯具未提供可读写的开关能力。"));
        }
        Ok(caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_lamp22_ids_and_ranges() {
        let caps = Capabilities::for_model("yeelink.light.lamp22");
        assert_eq!(caps.power.unwrap().piid, 1);
        let bright = caps.brightness.unwrap();
        assert_eq!(bright.piid, 2);
        assert_eq!(bright.range, Some([1, 100, 1]));
        let temp = caps.temperature.unwrap();
        assert_eq!((temp.siid, temp.piid), (2, 3));
        assert_eq!(temp.range, Some([2700, 6500, 1]));
        assert!(temp.validate(2699).is_err());
        assert!(temp.validate(6501).is_err());
        assert!(bright.validate(0).is_err());
        assert!(temp.validate(4321).is_ok());
    }
    #[test]
    fn unknown_model_never_assumes_color_temperature() {
        assert!(Capabilities::for_model("yeelink.light.mono")
            .temperature
            .is_none());
    }
    #[test]
    fn respects_spec_step_and_access() {
        let mut spec = serde_json::json!({"services": [{
            "iid": 2, "type": "urn:miot-spec-v2:service:light:00007802",
            "properties": [
                {"iid": 1, "type": "urn:miot-spec-v2:property:on:00000006", "access": ["read", "write"]},
                {"iid": 3, "type": "urn:miot-spec-v2:property:color-temperature:0000000F", "access": ["read", "write"], "value-range": [3000, 6000, 100]}
            ]
        }]});
        let caps = Capabilities::from_spec(&spec).unwrap();
        assert!(caps.temperature.unwrap().validate(3050).is_err());
        spec["services"][0]["properties"][1]["access"] = serde_json::json!(["read"]);
        assert!(Capabilities::from_spec(&spec)
            .unwrap()
            .temperature
            .is_none());
    }
}
