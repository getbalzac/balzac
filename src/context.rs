use crate::config;

pub fn merge_contexts(
    configuration: &config::Config,
    local_context: serde_json::Value,
) -> serde_json::Value {
    let global_value = configuration
        .global
        .as_ref()
        .map(|g| serde_json::to_value(g).unwrap())
        .unwrap_or(serde_json::json!({}));

    let mut merged = global_value;
    merge(&mut merged, &local_context);
    merged
}

fn merge(a: &mut serde_json::Value, b: &serde_json::Value) {
    if let (serde_json::Value::Object(a_map), serde_json::Value::Object(b_map)) = (a, b) {
        for (key, b_value) in b_map {
            match a_map.get(key) {
                Some(a_value) => {
                    if a_value.is_object() && b_value.is_object() {
                        merge(a_map.get_mut(key).unwrap(), b_value);
                    } else {
                        a_map.insert(key.clone(), b_value.clone());
                    }
                }
                None => {
                    a_map.insert(key.clone(), b_value.clone());
                }
            }
        }
    }
}
