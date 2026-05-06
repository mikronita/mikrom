use crate::github::generate_jwt;

#[test]
fn test_generate_jwt_with_escaped_newlines() {
    // Mock private key with escaped newlines (as it might come from an env var)
    let private_key_escaped =
        "-----BEGIN RSA PRIVATE KEY-----\\nABC\\n-----END RSA PRIVATE KEY-----";

    // This should fail to parse but we want to test if it cleans the string correctly.
    // Actually EncodingKey::from_rsa_pem will fail because ABC is not a valid key.
    // But we can check if it attempts to parse it after cleaning.

    let result = generate_jwt("123", private_key_escaped);

    match result {
        Err(e) => {
            let err_msg = e.to_string();
            // It should fail with "Invalid private key" and some details from jsonwebtoken,
            // not because it couldn't find the BEGIN/END markers due to escaped newlines.
            assert!(err_msg.contains("Invalid private key"));
        },
        Ok(_) => panic!("Should have failed"),
    }
}

#[test]
fn test_generate_jwt_with_quotes() {
    let private_key_quoted =
        "\"-----BEGIN RSA PRIVATE KEY-----\nABC\n-----END RSA PRIVATE KEY-----\"";
    let result = generate_jwt("123", private_key_quoted);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid private key")
    );
}
