pub const DEFAULT_DUCK: &str = include_str!("../tests/fixtures/duck.claydash");
pub fn fixture_objects(bytes: &[u8]) -> Vec<crate::model::SdfObject> {
    let document: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
        .unwrap()
}
