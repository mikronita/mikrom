package mikrom

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/base64"
	"errors"
)

func decrypt(encryptedData string, masterKey string) (string, error) {
	key := sha256.Sum256([]byte(masterKey))

	combined, err := base64.StdEncoding.DecodeString(encryptedData)
	if err != nil {
		return "", err
	}

	if len(combined) < 12 {
		return "", errors.New("invalid data length")
	}

	nonce := combined[:12]
	ciphertext := combined[12:]

	block, err := aes.NewCipher(key[:])
	if err != nil {
		return "", err
	}

	aesgcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}

	plaintext, err := aesgcm.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return "", err
	}

	return string(plaintext), nil
}
