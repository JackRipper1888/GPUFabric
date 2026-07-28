package support

import (
	"errors"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	Addr                 string
	Token                string
	CallbackSecret       string
	StateDir             string
	TrustRootExportPath  string
	ChromiumPath         string
	SoftHSMUtilPath      string
	PKCS11ToolPath       string
	PKCS11ModulePath     string
	SoftHSMConfigPath    string
	TokenLabel           string
	TokenUserPIN         string
	TokenSOPIN           string
	KeyID                string
	KeyLabel             string
	KeyVersion           string
	ClamAVAddr           string
	PDFToPPMPath         string
	TesseractPath        string
	AllowedDownloadHosts map[string]struct{}
	MaxHTMLBytes         int64
	MaxEvidenceBytes     int64
	RenderTimeout        time.Duration
	ScanTimeout          time.Duration
}

func LoadConfig() (Config, error) {
	renderTimeout, err := envDuration("E2E_RENDER_TIMEOUT", 60*time.Second)
	if err != nil {
		return Config{}, err
	}
	scanTimeout, err := envDuration("E2E_SCAN_TIMEOUT", 60*time.Second)
	if err != nil {
		return Config{}, err
	}
	maxHTML, err := envInt64("E2E_MAX_HTML_BYTES", 8<<20)
	if err != nil {
		return Config{}, err
	}
	maxEvidence, err := envInt64("E2E_MAX_EVIDENCE_BYTES", 16<<20)
	if err != nil {
		return Config{}, err
	}
	config := Config{
		Addr:                 envDefault("E2E_SUPPORT_ADDR", "127.0.0.1:28080"),
		Token:                strings.TrimSpace(os.Getenv("E2E_SUPPORT_TOKEN")),
		CallbackSecret:       strings.TrimSpace(os.Getenv("E2E_CALLBACK_SECRET")),
		StateDir:             envDefault("E2E_STATE_DIR", "/state"),
		TrustRootExportPath:  envDefault("E2E_TRUST_ROOT_EXPORT_PATH", "/public-trust/local-e2e-root-ca.pem"),
		ChromiumPath:         envDefault("E2E_CHROMIUM_PATH", "/usr/bin/chromium"),
		SoftHSMUtilPath:      envDefault("E2E_SOFTHSM_UTIL_PATH", "/usr/bin/softhsm2-util"),
		PKCS11ToolPath:       envDefault("E2E_PKCS11_TOOL_PATH", "/usr/bin/pkcs11-tool"),
		PKCS11ModulePath:     envDefault("E2E_PKCS11_MODULE_PATH", "/usr/lib/softhsm/libsofthsm2.so"),
		SoftHSMConfigPath:    envDefault("E2E_SOFTHSM_CONFIG_PATH", "/state/softhsm2.conf"),
		TokenLabel:           envDefault("E2E_HSM_TOKEN_LABEL", "gpuf-local-e2e"),
		TokenUserPIN:         strings.TrimSpace(os.Getenv("E2E_HSM_USER_PIN")),
		TokenSOPIN:           strings.TrimSpace(os.Getenv("E2E_HSM_SO_PIN")),
		KeyID:                envDefault("E2E_HSM_KEY_ID", "01"),
		KeyLabel:             envDefault("E2E_HSM_KEY_LABEL", "assessment-report-signing"),
		KeyVersion:           envDefault("E2E_HSM_KEY_VERSION", "local-e2e-p256-v1"),
		ClamAVAddr:           envDefault("E2E_CLAMAV_ADDR", "clamav:3310"),
		PDFToPPMPath:         envDefault("E2E_PDFTOPPM_PATH", "/usr/bin/pdftoppm"),
		TesseractPath:        envDefault("E2E_TESSERACT_PATH", "/usr/bin/tesseract"),
		AllowedDownloadHosts: parseHosts(envDefault("E2E_ALLOWED_DOWNLOAD_HOSTS", "minio:9000")),
		MaxHTMLBytes:         maxHTML,
		MaxEvidenceBytes:     maxEvidence,
		RenderTimeout:        renderTimeout,
		ScanTimeout:          scanTimeout,
	}
	if len(config.Token) < 32 || len(config.Token) > 4096 {
		return Config{}, errors.New("E2E_SUPPORT_TOKEN must contain 32 to 4096 bytes")
	}
	if len(config.CallbackSecret) < 32 || len(config.CallbackSecret) > 4096 {
		return Config{}, errors.New("E2E_CALLBACK_SECRET must contain 32 to 4096 bytes")
	}
	if config.TokenUserPIN == "" || config.TokenSOPIN == "" || config.StateDir == "" ||
		config.TrustRootExportPath == "" ||
		config.TokenLabel == "" || config.KeyID == "" || config.KeyLabel == "" ||
		config.KeyVersion == "" || len(config.AllowedDownloadHosts) == 0 {
		return Config{}, errors.New("local HSM identity and allowed download hosts must be configured")
	}
	return config, nil
}

func envDefault(name, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(name)); value != "" {
		return value
	}
	return fallback
}

func envDuration(name string, fallback time.Duration) (time.Duration, error) {
	raw := strings.TrimSpace(os.Getenv(name))
	if raw == "" {
		return fallback, nil
	}
	value, err := time.ParseDuration(raw)
	if err != nil || value <= 0 {
		return 0, errors.New(name + " must be a positive duration")
	}
	return value, nil
}

func envInt64(name string, fallback int64) (int64, error) {
	raw := strings.TrimSpace(os.Getenv(name))
	if raw == "" {
		return fallback, nil
	}
	value, err := strconv.ParseInt(raw, 10, 64)
	if err != nil || value <= 0 {
		return 0, errors.New(name + " must be a positive integer")
	}
	return value, nil
}

func parseHosts(raw string) map[string]struct{} {
	hosts := make(map[string]struct{})
	for _, item := range strings.Split(raw, ",") {
		if host := strings.ToLower(strings.TrimSpace(item)); host != "" {
			hosts[host] = struct{}{}
		}
	}
	return hosts
}
