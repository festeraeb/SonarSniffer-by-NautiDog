#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runargs_deserialize() {
        let v = json!({ "input": "a", "output": "b", "height": 720, "color": "blue" });
        let s = serde_json::to_string(&v).unwrap();
        let r: RunArgs = serde_json::from_str(&s).unwrap();
        assert_eq!(r.input, "a");
        assert_eq!(r.output, "b");
        assert_eq!(r.height, 720);
        assert_eq!(r.color, "blue");
    }
}
