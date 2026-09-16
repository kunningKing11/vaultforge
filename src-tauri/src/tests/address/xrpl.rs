use super::validate_address;

#[test]
fn accepts_classic_addresses_and_rejects_other_encodings() {
    assert!(validate_address("rG1QQv2nh2gr7RCZ1P8YYcBUKCCN633jCn").is_ok());
    assert!(validate_address("X7AcgcsBL6XDcUb289X4mJ8djcdyKaB5hJDWMArnXr61cqZ").is_err());
    assert!(validate_address("rG1QQv2nh2gr7RCZ1P8YYcBUKCCN633jCm").is_err());
}
