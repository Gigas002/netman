use super::*;

#[test]
fn mm_access_technology_maps_5g() {
    assert_eq!(
        mm_access_technology(MM_ACCESS_TECH_5GNR),
        AccessTechnology::Nr5G
    );
}

#[test]
fn mm_access_technology_maps_lte() {
    assert_eq!(
        mm_access_technology(MM_ACCESS_TECH_LTE),
        AccessTechnology::Lte
    );
}

#[test]
fn caps_to_access_technology_prefers_5g() {
    assert_eq!(
        caps_to_access_technology(NM_MODEM_CAP_5GNR | NM_MODEM_CAP_LTE),
        AccessTechnology::Nr5G
    );
}

#[test]
fn build_modem_info_reads_apn() {
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    let mut gsm = HashMap::new();
    gsm.insert(
        "apn".to_owned(),
        Value::from("internet").try_into().unwrap(),
    );
    let mut raw = HashMap::new();
    raw.insert("gsm".to_owned(), gsm);

    let info = build_modem_info(&raw);
    assert_eq!(info.apn.as_deref(), Some("internet"));
    assert!(!info.sim_locked);
}
