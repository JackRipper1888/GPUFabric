package e2erunner

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestLoadConfigLocalDefaults(t *testing.T) {
	clearRunnerEnvironment(t)
	t.Setenv("GPUF_TEST_BANKING_API_TOKEN", strings.Repeat("g", 32))

	config, err := LoadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if config.TenantRef != defaultTenantRef || config.SupportToken != defaultSupportToken {
		t.Fatalf("unexpected local defaults: tenant=%q support-token-length=%d", config.TenantRef, len(config.SupportToken))
	}
	if config.CallbackMode != callbackModeLocal || config.LifecycleMode != lifecycleModeFull {
		t.Fatalf("unexpected local modes: callback=%q lifecycle=%q", config.CallbackMode, config.LifecycleMode)
	}
	if len(config.Credentials) != len(defaultCredentials) {
		t.Fatalf("credential count = %d, want %d", len(config.Credentials), len(defaultCredentials))
	}
	config.Credentials["client"] = serviceCredential{}
	if defaultCredentials["client"].token == "" {
		t.Fatal("default credentials were not cloned")
	}
}

func TestLoadConfigSharedEnvironment(t *testing.T) {
	clearRunnerEnvironment(t)
	t.Setenv("ASSESSMENT_GPUFABRIC_TOKEN", strings.Repeat("g", 32))
	t.Setenv("E2E_ASSESSMENT_URL", "http://asset-assessment-service:8092")
	t.Setenv("E2E_SUPPORT_URL", "https://assessment-report-support")
	t.Setenv("E2E_GPUFABRIC_URL", "http://gpuf-api-server:18081")
	t.Setenv("E2E_ALLOW_CONTAINER_HTTP", "true")
	t.Setenv("E2E_SUPPORT_TOKEN", strings.Repeat("s", 32))
	t.Setenv("E2E_CALLBACK_MODE", callbackModeExternal)
	t.Setenv("E2E_REPORT_LIFECYCLE_MODE", lifecycleModeSkip)
	t.Setenv("E2E_TENANT_REF", "tenant-shared-test")
	t.Setenv("E2E_ASSESSMENT_CREDENTIALS_JSON", sharedCredentialJSON(t))

	config, err := LoadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if config.CallbackMode != callbackModeExternal || config.LifecycleMode != lifecycleModeSkip {
		t.Fatalf("unexpected shared modes: callback=%q lifecycle=%q", config.CallbackMode, config.LifecycleMode)
	}
	if config.TenantRef != "tenant-shared-test" {
		t.Fatalf("tenant = %q", config.TenantRef)
	}
	expectedClientToken := "new-api-current-token-" + strings.Repeat("x", 32)
	if config.Credentials["client"].subject != "new-api" || config.Credentials["client"].token != expectedClientToken {
		t.Fatalf("client credential mapping is incorrect: subject=%q token-length=%d", config.Credentials["client"].subject, len(config.Credentials["client"].token))
	}
	if _, exists := config.Credentials["revoke"]; exists {
		t.Fatal("skip lifecycle mode unexpectedly requires revocation credentials")
	}
}

func TestLoadConfigExistingAssessmentSharedMode(t *testing.T) {
	setSharedRunnerEnvironment(t)
	t.Setenv("E2E_EXISTING_ASSESSMENT_ID", "ASMT-20260814-36289115ce03fc64")

	config, err := LoadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if config.ExistingAssessmentID != "ASMT-20260814-36289115ce03fc64" {
		t.Fatalf("existing assessment = %q", config.ExistingAssessmentID)
	}
}

func TestLoadConfigExistingAssessmentFailsClosed(t *testing.T) {
	for _, invalid := range []string{"assessment-1", "ASMT-../../etc", " ASMT-1", "ASMT-1 ", "ASMT-" + strings.Repeat("x", 124)} {
		t.Run(invalid, func(t *testing.T) {
			setSharedRunnerEnvironment(t)
			t.Setenv("E2E_EXISTING_ASSESSMENT_ID", invalid)
			if _, err := LoadConfig(); err == nil {
				t.Fatalf("invalid existing assessment %q was accepted", invalid)
			}
		})
	}
	t.Run("full lifecycle", func(t *testing.T) {
		setSharedRunnerEnvironment(t)
		t.Setenv("E2E_REPORT_LIFECYCLE_MODE", lifecycleModeFull)
		t.Setenv("E2E_EXISTING_ASSESSMENT_ID", "ASMT-existing-1")
		if _, err := LoadConfig(); err == nil {
			t.Fatal("existing assessment with full lifecycle was accepted")
		}
	})
	t.Run("implicit tenant", func(t *testing.T) {
		setSharedRunnerEnvironment(t)
		t.Setenv("E2E_TENANT_REF", "")
		t.Setenv("E2E_EXISTING_ASSESSMENT_ID", "ASMT-existing-1")
		if _, err := LoadConfig(); err == nil {
			t.Fatal("existing assessment without an explicit tenant was accepted")
		}
	})
}

func TestLoadRunnerCredentialsRejectsInvalidConfiguration(t *testing.T) {
	clearRunnerEnvironment(t)
	if _, err := loadRunnerCredentials(`{"new-api":{"token":"client-token"}}`, lifecycleModeSkip); err == nil {
		t.Fatal("missing required subjects were accepted")
	}

	var configured map[string]configuredCredential
	if err := json.Unmarshal([]byte(sharedCredentialJSON(t)), &configured); err != nil {
		t.Fatal(err)
	}
	configured["report-issue-worker"] = configuredCredential{}
	raw, err := json.Marshal(configured)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := loadRunnerCredentials(string(raw), lifecycleModeSkip); err == nil {
		t.Fatal("credential without a token was accepted")
	}
	configured["report-issue-worker"] = configuredCredential{Token: strings.Repeat("x", 4097)}
	raw, err = json.Marshal(configured)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := loadRunnerCredentials(string(raw), lifecycleModeSkip); err == nil {
		t.Fatal("oversized credential token was accepted")
	}
}

func TestValidateServiceURLRestrictions(t *testing.T) {
	tests := []struct {
		name, raw string
		allow     bool
		wantError bool
	}{
		{name: "loopback", raw: "http://127.0.0.1:8092"},
		{name: "localhost", raw: "http://localhost:8092"},
		{name: "container denied by default", raw: "http://asset-assessment-service:8092", wantError: true},
		{name: "container explicitly allowed", raw: "http://asset-assessment-service:8092", allow: true},
		{name: "private address explicitly allowed", raw: "http://172.18.0.3:8092", allow: true},
		{name: "public HTTP denied", raw: "http://8.8.8.8:8092", allow: true, wantError: true},
		{name: "TLS allowed", raw: "https://assessment-report-support"},
		{name: "credentials denied", raw: "https://user:pass@example.test", wantError: true},
		{name: "query denied", raw: "https://example.test?token=value", wantError: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			err := validateServiceURL(test.raw, test.allow)
			if (err != nil) != test.wantError {
				t.Fatalf("validateServiceURL(%q, %v) error = %v", test.raw, test.allow, err)
			}
		})
	}
}

func TestLoadConfigRejectsSkipLifecycleWithLocalCallback(t *testing.T) {
	clearRunnerEnvironment(t)
	t.Setenv("GPUF_TEST_BANKING_API_TOKEN", strings.Repeat("g", 32))
	t.Setenv("E2E_REPORT_LIFECYCLE_MODE", lifecycleModeSkip)
	if _, err := LoadConfig(); err == nil {
		t.Fatal("skip lifecycle mode with local callback was accepted")
	}
}

func sharedCredentialJSON(t *testing.T) string {
	t.Helper()
	configured := make(map[string]configuredCredential)
	for role, subjects := range credentialSubjectCandidates {
		if role == "revoke" || role == "expiry" {
			continue
		}
		subject := subjects[0]
		if role == "client" {
			subject = "new-api"
		}
		configured[subject] = configuredCredential{Tokens: []string{
			subject + "-current-token-" + strings.Repeat("x", 32),
			subject + "-next-token-" + strings.Repeat("y", 32),
		}}
	}
	raw, err := json.Marshal(configured)
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func setSharedRunnerEnvironment(t *testing.T) {
	t.Helper()
	clearRunnerEnvironment(t)
	t.Setenv("ASSESSMENT_GPUFABRIC_TOKEN", strings.Repeat("g", 32))
	t.Setenv("E2E_ASSESSMENT_URL", "http://asset-assessment-service:8092")
	t.Setenv("E2E_SUPPORT_URL", "https://assessment-report-support")
	t.Setenv("E2E_GPUFABRIC_URL", "http://gpuf-api-server:18081")
	t.Setenv("E2E_ALLOW_CONTAINER_HTTP", "true")
	t.Setenv("E2E_SUPPORT_TOKEN", strings.Repeat("s", 32))
	t.Setenv("E2E_CALLBACK_MODE", callbackModeExternal)
	t.Setenv("E2E_REPORT_LIFECYCLE_MODE", lifecycleModeSkip)
	t.Setenv("E2E_TENANT_REF", "tenant-shared-test")
	t.Setenv("E2E_ASSESSMENT_CREDENTIALS_JSON", sharedCredentialJSON(t))
}

func clearRunnerEnvironment(t *testing.T) {
	t.Helper()
	for _, name := range []string{
		"GPUF_TEST_BANKING_API_TOKEN", "ASSESSMENT_GPUFABRIC_TOKEN",
		"E2E_ASSESSMENT_URL", "E2E_SUPPORT_URL", "E2E_GPUFABRIC_URL", "E2E_ALLOW_CONTAINER_HTTP",
		"E2E_SUPPORT_TOKEN", "ASSESSMENT_PDF_RENDERER_TOKEN",
		"E2E_CALLBACK_MODE", "ASSESSMENT_E2E_CALLBACK_SECRET", "ASSESSMENT_CALLBACK_SIGNING_SECRET",
		"E2E_REPORT_LIFECYCLE_MODE", "E2E_TENANT_REF",
		"E2E_EXISTING_ASSESSMENT_ID",
		"E2E_ASSESSMENT_CREDENTIALS_JSON", "ASSESSMENT_SERVICE_CREDENTIALS_JSON",
		"E2E_ASSESSMENT_SERVICE_TOKEN", "ASSESSMENT_SERVICE_TOKEN",
		"E2E_ASSESSMENT_LEGACY_SERVICE_SUBJECT", "ASSESSMENT_LEGACY_SERVICE_SUBJECT",
	} {
		t.Setenv(name, "")
	}
}
