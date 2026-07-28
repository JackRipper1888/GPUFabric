package support

import (
	"context"
	"crypto"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/asn1"
	"encoding/base64"
	"encoding/hex"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"math/big"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const reportSignatureSchema = "asset_assessment.report-signature.v1"

type ReportSignature struct {
	SchemaVersion          string    `json:"schemaVersion"`
	Algorithm              string    `json:"algorithm"`
	KeyVersion             string    `json:"keyVersion"`
	CertificateChain       []string  `json:"certificateChain"`
	CertificateChainSHA256 string    `json:"certificateChainSha256"`
	SigningDigestSHA256    string    `json:"signingDigestSha256"`
	Signature              string    `json:"signature"`
	TimestampAuthority     string    `json:"timestampAuthority"`
	TimestampTokenSHA256   string    `json:"timestampTokenSha256"`
	TimestampToken         string    `json:"timestampToken"`
	SignedAt               time.Time `json:"signedAt"`
}

type ReportSigner interface {
	Sign(ctx context.Context, digestSHA256 string) (ReportSignature, error)
	PublicInfo() TrustInfo
}

type TrustInfo struct {
	Mode             string   `json:"mode"`
	Production       bool     `json:"production"`
	Algorithm        string   `json:"algorithm"`
	KeyVersion       string   `json:"keyVersion"`
	CertificateChain []string `json:"certificateChain"`
	TrustRootPath    string   `json:"trustRootPath"`
}

type PKCS11Signer struct {
	config      Config
	runner      commandRunner
	publicKey   *ecdsa.PublicKey
	chain       []string
	trustRoot   string
	publicRoot  string
	environment []string
}

func InitializePKCS11Signer(ctx context.Context, config Config) (*PKCS11Signer, error) {
	return initializePKCS11Signer(ctx, config, execCommandRunner{})
}

func initializePKCS11Signer(ctx context.Context, config Config, runner commandRunner) (*PKCS11Signer, error) {
	tokenDir := filepath.Join(config.StateDir, "tokens")
	if err := os.MkdirAll(tokenDir, 0o700); err != nil {
		return nil, err
	}
	if err := os.MkdirAll(config.StateDir, 0o700); err != nil {
		return nil, err
	}
	softHSMConfig := "directories.tokendir = " + tokenDir + "\nobjectstore.backend = file\nlog.level = ERROR\nslots.removable = false\n"
	if err := os.WriteFile(config.SoftHSMConfigPath, []byte(softHSMConfig), 0o600); err != nil {
		return nil, err
	}
	signer := &PKCS11Signer{
		config: config, runner: runner,
		environment: []string{"SOFTHSM2_CONF=" + config.SoftHSMConfigPath},
		trustRoot:   filepath.Join(config.StateDir, "local-e2e-root-ca.pem"),
	}
	leafPath := filepath.Join(config.StateDir, "local-e2e-report-signer.pem")
	if err := signer.ensureTokenAndKey(ctx, leafPath); err != nil {
		return nil, err
	}
	leafPEM, err := os.ReadFile(leafPath)
	if err != nil {
		return nil, err
	}
	rootPEM, err := os.ReadFile(signer.trustRoot)
	if err != nil {
		return nil, err
	}
	if err := publishTrustRoot(config.TrustRootExportPath, rootPEM); err != nil {
		return nil, fmt.Errorf("publish local trust root: %w", err)
	}
	signer.publicRoot = config.TrustRootExportPath
	leaf, err := parseCertificate(leafPEM)
	if err != nil {
		return nil, fmt.Errorf("parse local leaf certificate: %w", err)
	}
	publicKey, ok := leaf.PublicKey.(*ecdsa.PublicKey)
	if !ok || publicKey.Curve != elliptic.P256() {
		return nil, errors.New("local HSM leaf certificate does not contain a P-256 key")
	}
	signer.publicKey = publicKey
	signer.chain = []string{strings.TrimSpace(string(leafPEM)), strings.TrimSpace(string(rootPEM))}
	probe := sha256.Sum256([]byte("gpuf-local-e2e-hsm-possession-check"))
	signature, err := signer.signDigest(ctx, probe[:])
	if err != nil {
		return nil, fmt.Errorf("local HSM possession check: %w", err)
	}
	if !ecdsa.VerifyASN1(publicKey, probe[:], signature) {
		return nil, errors.New("local HSM possession signature did not verify")
	}
	return signer, nil
}

func publishTrustRoot(path string, encoded []byte) error {
	if _, err := parseCertificate(encoded); err != nil {
		return err
	}
	directory := filepath.Dir(path)
	if err := os.MkdirAll(directory, 0o755); err != nil {
		return err
	}
	temporary := path + ".tmp"
	if err := os.WriteFile(temporary, encoded, 0o644); err != nil {
		return err
	}
	if err := os.Chmod(temporary, 0o644); err != nil {
		return err
	}
	return os.Rename(temporary, path)
}

func (signer *PKCS11Signer) ensureTokenAndKey(ctx context.Context, leafPath string) error {
	slots, err := signer.runner.Run(ctx, signer.environment, nil, signer.config.SoftHSMUtilPath, "--show-slots")
	if err != nil {
		return err
	}
	if !strings.Contains(string(slots), "Label:            "+signer.config.TokenLabel) &&
		!strings.Contains(string(slots), "Label: "+signer.config.TokenLabel) {
		if _, err := signer.runner.Run(ctx, signer.environment, nil, signer.config.SoftHSMUtilPath,
			"--init-token", "--free", "--label", signer.config.TokenLabel,
			"--so-pin", signer.config.TokenSOPIN, "--pin", signer.config.TokenUserPIN); err != nil {
			return err
		}
	}
	if _, err := os.Stat(leafPath); err == nil {
		return nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if _, err := signer.runner.Run(ctx, signer.environment, nil, signer.config.PKCS11ToolPath,
		"--module", signer.config.PKCS11ModulePath, "--token-label", signer.config.TokenLabel,
		"--login", "--pin", signer.config.TokenUserPIN, "--keypairgen",
		"--key-type", "EC:prime256v1", "--usage-sign", "--id", signer.config.KeyID,
		"--label", signer.config.KeyLabel); err != nil {
		return err
	}
	publicDERPath := filepath.Join(signer.config.StateDir, "local-e2e-hsm-public.der")
	if _, err := signer.runner.Run(ctx, signer.environment, nil, signer.config.PKCS11ToolPath,
		"--module", signer.config.PKCS11ModulePath, "--token-label", signer.config.TokenLabel,
		"--login", "--pin", signer.config.TokenUserPIN, "--read-object", "--type", "pubkey",
		"--id", signer.config.KeyID, "--output-file", publicDERPath); err != nil {
		return err
	}
	publicDER, err := os.ReadFile(publicDERPath)
	if err != nil {
		return err
	}
	publicKey, err := parseP256PublicKey(publicDER)
	if err != nil {
		return fmt.Errorf("parse HSM public key: %w", err)
	}
	return signer.issueLocalCertificates(publicKey, leafPath)
}

func (signer *PKCS11Signer) issueLocalCertificates(publicKey *ecdsa.PublicKey, leafPath string) error {
	rootKeyPath := filepath.Join(signer.config.StateDir, "local-e2e-root-ca-key.pem")
	rootKey, rootCertificate, err := ensureLocalRoot(rootKeyPath, signer.trustRoot)
	if err != nil {
		return err
	}
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 120))
	if err != nil {
		return err
	}
	now := time.Now().UTC()
	template := &x509.Certificate{
		SerialNumber: serial,
		Subject: pkix.Name{
			CommonName:   "GPUFabric Local E2E Report Signer",
			Organization: []string{"NON-PRODUCTION LOCAL TEST"},
		},
		NotBefore: now.Add(-time.Hour),
		NotAfter:  now.AddDate(5, 0, 0),
		KeyUsage:  x509.KeyUsageDigitalSignature,
		ExtKeyUsage: []x509.ExtKeyUsage{
			x509.ExtKeyUsageCodeSigning,
		},
		BasicConstraintsValid: true,
	}
	der, err := x509.CreateCertificate(rand.Reader, template, rootCertificate, publicKey, rootKey)
	if err != nil {
		return err
	}
	return writePEMAtomic(leafPath, "CERTIFICATE", der, 0o644)
}

func ensureLocalRoot(keyPath, certificatePath string) (*ecdsa.PrivateKey, *x509.Certificate, error) {
	keyPEM, keyErr := os.ReadFile(keyPath)
	certificatePEM, certificateErr := os.ReadFile(certificatePath)
	if keyErr == nil && certificateErr == nil {
		block, _ := pem.Decode(keyPEM)
		if block == nil {
			return nil, nil, errors.New("local root private key PEM is invalid")
		}
		key, err := x509.ParseECPrivateKey(block.Bytes)
		if err != nil {
			return nil, nil, err
		}
		certificate, err := parseCertificate(certificatePEM)
		return key, certificate, err
	}
	if (!errors.Is(keyErr, os.ErrNotExist) && keyErr != nil) ||
		(!errors.Is(certificateErr, os.ErrNotExist) && certificateErr != nil) {
		return nil, nil, errors.New("local root identity is incomplete")
	}
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return nil, nil, err
	}
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 120))
	if err != nil {
		return nil, nil, err
	}
	now := time.Now().UTC()
	template := &x509.Certificate{
		SerialNumber: serial,
		Subject: pkix.Name{
			CommonName:   "GPUFabric Local E2E Root CA",
			Organization: []string{"NON-PRODUCTION LOCAL TEST"},
		},
		NotBefore:             now.Add(-time.Hour),
		NotAfter:              now.AddDate(10, 0, 0),
		IsCA:                  true,
		KeyUsage:              x509.KeyUsageCertSign | x509.KeyUsageCRLSign,
		BasicConstraintsValid: true,
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		return nil, nil, err
	}
	keyDER, err := x509.MarshalECPrivateKey(key)
	if err != nil {
		return nil, nil, err
	}
	if err := writePEMAtomic(keyPath, "EC PRIVATE KEY", keyDER, 0o600); err != nil {
		return nil, nil, err
	}
	if err := writePEMAtomic(certificatePath, "CERTIFICATE", der, 0o644); err != nil {
		return nil, nil, err
	}
	certificate, err := x509.ParseCertificate(der)
	return key, certificate, err
}

func writePEMAtomic(path, blockType string, der []byte, mode os.FileMode) error {
	temporary := path + ".tmp"
	encoded := pem.EncodeToMemory(&pem.Block{Type: blockType, Bytes: der})
	if err := os.WriteFile(temporary, encoded, mode); err != nil {
		return err
	}
	if err := os.Chmod(temporary, mode); err != nil {
		return err
	}
	return os.Rename(temporary, path)
}

func parseCertificate(encoded []byte) (*x509.Certificate, error) {
	block, _ := pem.Decode(encoded)
	if block == nil || block.Type != "CERTIFICATE" {
		return nil, errors.New("certificate PEM is invalid")
	}
	return x509.ParseCertificate(block.Bytes)
}

func parseP256PublicKey(encoded []byte) (*ecdsa.PublicKey, error) {
	if parsed, err := x509.ParsePKIXPublicKey(encoded); err == nil {
		if publicKey, ok := parsed.(*ecdsa.PublicKey); ok && publicKey.Curve == elliptic.P256() {
			return publicKey, nil
		}
	}
	point := encoded
	var octets []byte
	if rest, err := asn1.Unmarshal(encoded, &octets); err == nil && len(rest) == 0 {
		point = octets
	}
	if len(point) == 67 && point[0] == 0x04 && point[1] == 65 {
		point = point[2:]
	}
	x, y := elliptic.Unmarshal(elliptic.P256(), point)
	if x == nil || y == nil {
		return nil, errors.New("unsupported P-256 public key encoding")
	}
	return &ecdsa.PublicKey{Curve: elliptic.P256(), X: x, Y: y}, nil
}

func (signer *PKCS11Signer) Sign(ctx context.Context, digestSHA256 string) (ReportSignature, error) {
	digest, err := hex.DecodeString(strings.ToLower(strings.TrimSpace(digestSHA256)))
	if err != nil || len(digest) != sha256.Size {
		return ReportSignature{}, errors.New("signing digest must be a SHA-256 value")
	}
	signatureDER, err := signer.signDigest(ctx, digest)
	if err != nil {
		return ReportSignature{}, err
	}
	if !ecdsa.VerifyASN1(signer.publicKey, digest, signatureDER) {
		return ReportSignature{}, errors.New("HSM signature did not verify against the issued certificate")
	}
	signedAt := time.Now().UTC()
	timestampToken := []byte("NON_PRODUCTION_LOCAL_TIMESTAMP\n" + digestSHA256 + "\n" + signedAt.Format(time.RFC3339Nano))
	timestampHash := sha256.Sum256(timestampToken)
	chainHash := sha256.Sum256([]byte(strings.Join(signer.chain, "\n")))
	return ReportSignature{
		SchemaVersion:          reportSignatureSchema,
		Algorithm:              "ECDSA-P256-SHA256",
		KeyVersion:             signer.config.KeyVersion,
		CertificateChain:       append([]string(nil), signer.chain...),
		CertificateChainSHA256: hex.EncodeToString(chainHash[:]),
		SigningDigestSHA256:    strings.ToLower(digestSHA256),
		Signature:              base64.StdEncoding.EncodeToString(signatureDER),
		TimestampAuthority:     "local-e2e-untrusted-timestamp",
		TimestampTokenSHA256:   hex.EncodeToString(timestampHash[:]),
		TimestampToken:         base64.StdEncoding.EncodeToString(timestampToken),
		SignedAt:               signedAt,
	}, nil
}

func (signer *PKCS11Signer) signDigest(ctx context.Context, digest []byte) ([]byte, error) {
	directory, err := os.MkdirTemp(signer.config.StateDir, "sign-")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(directory)
	inputPath := filepath.Join(directory, "digest.bin")
	outputPath := filepath.Join(directory, "signature.bin")
	if err := os.WriteFile(inputPath, digest, 0o600); err != nil {
		return nil, err
	}
	if _, err := signer.runner.Run(ctx, signer.environment, nil, signer.config.PKCS11ToolPath,
		"--module", signer.config.PKCS11ModulePath, "--token-label", signer.config.TokenLabel,
		"--login", "--pin", signer.config.TokenUserPIN, "--sign", "--mechanism", "ECDSA",
		"--id", signer.config.KeyID, "--input-file", inputPath, "--output-file", outputPath); err != nil {
		return nil, err
	}
	raw, err := os.ReadFile(outputPath)
	if err != nil {
		return nil, err
	}
	return normalizeECDSASignature(raw)
}

func normalizeECDSASignature(raw []byte) ([]byte, error) {
	var parsed struct{ R, S *big.Int }
	if rest, err := asn1.Unmarshal(raw, &parsed); err == nil && len(rest) == 0 &&
		parsed.R != nil && parsed.S != nil {
		return raw, nil
	}
	if len(raw) == 0 || len(raw)%2 != 0 {
		return nil, errors.New("PKCS#11 returned an invalid ECDSA signature")
	}
	half := len(raw) / 2
	return asn1.Marshal(struct{ R, S *big.Int }{
		R: new(big.Int).SetBytes(raw[:half]),
		S: new(big.Int).SetBytes(raw[half:]),
	})
}

func (signer *PKCS11Signer) PublicInfo() TrustInfo {
	return TrustInfo{
		Mode: "softhsm2-local-test", Production: false, Algorithm: "ECDSA-P256-SHA256",
		KeyVersion: signer.config.KeyVersion, CertificateChain: append([]string(nil), signer.chain...),
		TrustRootPath: signer.publicRoot,
	}
}

type pkcs11CryptoSigner struct {
	publicKey *ecdsa.PublicKey
	sign      func(context.Context, []byte) ([]byte, error)
	context   context.Context
}

func (signer pkcs11CryptoSigner) Public() crypto.PublicKey { return signer.publicKey }

func (signer pkcs11CryptoSigner) Sign(_ io.Reader, digest []byte, _ crypto.SignerOpts) ([]byte, error) {
	return signer.sign(signer.context, digest)
}
