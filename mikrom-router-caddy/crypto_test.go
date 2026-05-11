package mikrom

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/base64"
	"testing"
)

func encryptGo(data, masterKey string) (string, error) {
	key := sha256.Sum256([]byte(masterKey))
	block, err := aes.NewCipher(key[:])
	if err != nil {
		return "", err
	}

	aesgcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}

	nonce := make([]byte, 12) // In production use rand.Read, but for tests we just need consistency
	// In the Rust code, they use OsRng to generate the nonce.
	// For testing decryption, we just need to know the nonce used for encryption.

	ciphertext := aesgcm.Seal(nil, nonce, []byte(data), nil)

	combined := append(nonce, ciphertext...)
	return base64.StdEncoding.EncodeToString(combined), nil
}

func TestDecrypt(t *testing.T) {
	masterKey := "super-secret-key"
	originalText := "hello-mikrom"

	// Test successful decryption
	encrypted, err := encryptGo(originalText, masterKey)
	if err != nil {
		t.Fatalf("failed to encrypt: %v", err)
	}

	decrypted, err := decrypt(encrypted, masterKey)
	if err != nil {
		t.Fatalf("failed to decrypt: %v", err)
	}

	if decrypted != originalText {
		t.Errorf("expected %q, got %q", originalText, decrypted)
	}

	// Test invalid master key
	_, err = decrypt(encrypted, "wrong-key")
	if err == nil {
		t.Error("expected error with wrong key, got nil")
	}

	// Test invalid base64
	_, err = decrypt("not-base64-!", masterKey)
	if err == nil {
		t.Error("expected error with invalid base64, got nil")
	}

	// Test too short data
	_, err = decrypt(base64.StdEncoding.EncodeToString([]byte("short")), masterKey)
	if err == nil {
		t.Error("expected error with short data, got nil")
	}
}
