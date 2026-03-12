use serde::{Deserialize, Serialize};

/// A physical address following the W3C Contact Picker API.
///
/// All fields are optional to accommodate partial address information.
///
/// See: <https://www.w3.org/TR/contact-picker/#contact-address>
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContactAddress {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependent_locality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sorting_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_line: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_full() {
        let addr = ContactAddress {
            city: Some("San Francisco".into()),
            country: Some("US".into()),
            dependent_locality: None,
            organization: Some("Acme Corp".into()),
            phone_number: Some("+1-555-0100".into()),
            postal_code: Some("94105".into()),
            recipient: Some("Alice Smith".into()),
            region: Some("CA".into()),
            sorting_code: None,
            address_line: Some(vec!["123 Market St".into(), "Suite 400".into()]),
        };
        let json = serde_json::to_string(&addr).unwrap();
        let back: ContactAddress = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, back);
    }

    #[test]
    fn roundtrip_empty() {
        let addr = ContactAddress::default();
        let json = serde_json::to_string(&addr).unwrap();
        assert_eq!(json, "{}");
        let back: ContactAddress = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, back);
    }
}
