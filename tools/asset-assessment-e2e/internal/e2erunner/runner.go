package e2erunner

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	defaultSupportToken  = "local-e2e-support-token-only-rotate-before-production"
	defaultTenantRef     = "tenant-local-e2e"
	callbackModeLocal    = "local"
	callbackModeExternal = "external"
	lifecycleModeFull    = "full"
	lifecycleModeSkip    = "skip"
)

type Config struct {
	AssessmentURL   string
	SupportURL      string
	SupportToken    string
	GPUFabricURL    string
	GPUFabricToken  string
	GPUFabricUser   string
	GPUFabricClient string
	TenantRef       string
	Credentials     map[string]serviceCredential
	CallbackMode    string
	CallbackSecret  string
	LifecycleMode   string
	FixtureDir      string
	OutputDir       string
}

func LoadConfig() (Config, error) {
	allowContainerHTTP, err := envBool("E2E_ALLOW_CONTAINER_HTTP", false)
	if err != nil {
		return Config{}, err
	}
	callbackMode := envDefault("E2E_CALLBACK_MODE", callbackModeLocal)
	if callbackMode != callbackModeLocal && callbackMode != callbackModeExternal {
		return Config{}, errors.New("E2E_CALLBACK_MODE must be local or external")
	}
	lifecycleMode := envDefault("E2E_REPORT_LIFECYCLE_MODE", lifecycleModeFull)
	if lifecycleMode != lifecycleModeFull && lifecycleMode != lifecycleModeSkip {
		return Config{}, errors.New("E2E_REPORT_LIFECYCLE_MODE must be full or skip")
	}
	if lifecycleMode == lifecycleModeSkip && callbackMode != callbackModeExternal {
		return Config{}, errors.New("skipping report lifecycle gates requires E2E_CALLBACK_MODE=external")
	}

	config := Config{
		AssessmentURL:   envDefault("E2E_ASSESSMENT_URL", "http://127.0.0.1:28092"),
		SupportURL:      envDefault("E2E_SUPPORT_URL", "http://127.0.0.1:28180"),
		GPUFabricURL:    envDefault("E2E_GPUFABRIC_URL", "http://127.0.0.1:18181"),
		GPUFabricToken:  firstEnvironment("GPUF_TEST_BANKING_API_TOKEN", "ASSESSMENT_GPUFABRIC_TOKEN"),
		GPUFabricUser:   envDefault("E2E_GPUFABRIC_USER_REF", "1"),
		GPUFabricClient: envDefault("E2E_GPUFABRIC_CLIENT_REF", "bcbe19cbf6063d72f8253d22abad8bb6"),
		TenantRef:       envDefault("E2E_TENANT_REF", defaultTenantRef),
		CallbackMode:    callbackMode,
		CallbackSecret:  firstEnvironment("ASSESSMENT_E2E_CALLBACK_SECRET", "ASSESSMENT_CALLBACK_SIGNING_SECRET"),
		LifecycleMode:   lifecycleMode,
		FixtureDir:      envDefault("E2E_MARKET_FIXTURE_DIR", "deploy/fixtures/market"),
		OutputDir:       envDefault("E2E_OUTPUT_DIR", "/tmp/gpuf-asset-assessment-e2e-results"),
	}
	if len(config.GPUFabricToken) < 32 || len(config.GPUFabricToken) > 4096 {
		return Config{}, errors.New("GPUF_TEST_BANKING_API_TOKEN or ASSESSMENT_GPUFABRIC_TOKEN must contain 32 to 4096 bytes")
	}
	allLocal := true
	for _, raw := range []string{config.AssessmentURL, config.SupportURL, config.GPUFabricURL} {
		if err := validateServiceURL(raw, allowContainerHTTP); err != nil {
			return Config{}, err
		}
		if !isLoopbackHTTP(raw) {
			allLocal = false
		}
	}
	config.SupportToken = firstEnvironment("E2E_SUPPORT_TOKEN", "ASSESSMENT_PDF_RENDERER_TOKEN")
	if config.SupportToken == "" && allLocal {
		config.SupportToken = defaultSupportToken
	}
	if len(config.SupportToken) < 32 || len(config.SupportToken) > 4096 {
		return Config{}, errors.New("E2E_SUPPORT_TOKEN or ASSESSMENT_PDF_RENDERER_TOKEN must contain 32 to 4096 bytes")
	}
	if config.CallbackMode == callbackModeLocal && config.CallbackSecret == "" && allLocal {
		config.CallbackSecret = "local-e2e-assessment-callback-secret-only"
	}
	if config.CallbackMode == callbackModeLocal && (len(config.CallbackSecret) < 32 || len(config.CallbackSecret) > 4096) {
		return Config{}, errors.New("ASSESSMENT_E2E_CALLBACK_SECRET must contain 32 to 4096 bytes")
	}
	credentialJSON := firstEnvironment("E2E_ASSESSMENT_CREDENTIALS_JSON", "ASSESSMENT_SERVICE_CREDENTIALS_JSON")
	if credentialJSON == "" && !allLocal {
		return Config{}, errors.New("shared E2E requires E2E_ASSESSMENT_CREDENTIALS_JSON or ASSESSMENT_SERVICE_CREDENTIALS_JSON")
	}
	config.Credentials, err = loadRunnerCredentials(credentialJSON, config.LifecycleMode)
	if err != nil {
		return Config{}, err
	}
	return config, nil
}

type runner struct {
	config    Config
	client    *http.Client
	runID     string
	phase     string
	startedAt time.Time
}

type serviceCredential struct{ subject, token string }

var defaultCredentials = map[string]serviceCredential{
	"client":         {"e2e-client", "local-e2e-client-token-0000000000000001"},
	"storage":        {"object-storage-gateway", "local-e2e-storage-token-00000000000001"},
	"scanner":        {"evidence-scanner", "local-e2e-scanner-token-00000000000001"},
	"evidence":       {"assessment-reviewer", "local-e2e-evidence-review-token-000001"},
	"market-a":       {"market-provider-a", "local-e2e-market-a-token-0000000000001"},
	"market-b":       {"market-provider-b", "local-e2e-market-b-token-0000000000001"},
	"market-verify":  {"market-data-verifier", "local-e2e-market-verify-token-000000001"},
	"snapshot":       {"market-snapshot-worker", "local-e2e-market-snapshot-token-0000001"},
	"policy-author":  {"pricing-policy-author", "local-e2e-pricing-author-token-00000001"},
	"policy-approve": {"pricing-policy-approver", "local-e2e-pricing-approve-token-0000001"},
	"valuation":      {"valuation-worker", "local-e2e-valuation-token-000000000001"},
	"coordinator":    {"formal-review-coordinator", "local-e2e-review-coordinate-token-00001"},
	"workbench":      {"formal-review-workbench", "local-e2e-review-workbench-token-00001"},
	"freeze":         {"report-freeze-worker", "local-e2e-report-freeze-token-0000001"},
	"issue":          {"report-issue-worker", "local-e2e-report-issue-token-00000001"},
	"download":       {"report-download-gateway", "local-e2e-report-download-token-00001"},
	"revoke":         {"report-revoke-worker", "local-e2e-report-revoke-token-0000001"},
	"expiry":         {"report-expiry-worker", "local-e2e-report-expiry-token-0000001"},
}

var credentialSubjectCandidates = map[string][]string{
	"client":         {"e2e-client", "new-api"},
	"storage":        {"object-storage-gateway"},
	"scanner":        {"evidence-scanner"},
	"evidence":       {"assessment-reviewer"},
	"market-a":       {"market-provider-a"},
	"market-b":       {"market-provider-b"},
	"market-verify":  {"market-data-verifier"},
	"snapshot":       {"market-snapshot-worker"},
	"policy-author":  {"pricing-policy-author"},
	"policy-approve": {"pricing-policy-approver"},
	"valuation":      {"valuation-worker"},
	"coordinator":    {"formal-review-coordinator"},
	"workbench":      {"formal-review-workbench"},
	"freeze":         {"report-freeze-worker"},
	"issue":          {"report-issue-worker"},
	"download":       {"report-download-gateway"},
	"revoke":         {"report-revoke-worker"},
	"expiry":         {"report-expiry-worker"},
}

type configuredCredential struct {
	Token  string   `json:"token"`
	Tokens []string `json:"tokens"`
}

func loadRunnerCredentials(raw, lifecycleMode string) (map[string]serviceCredential, error) {
	if strings.TrimSpace(raw) == "" {
		return cloneCredentials(defaultCredentials), nil
	}
	configured := make(map[string]configuredCredential)
	if err := json.Unmarshal([]byte(raw), &configured); err != nil {
		return nil, errors.New("E2E assessment credentials must be valid JSON")
	}
	legacyToken := firstEnvironment("E2E_ASSESSMENT_SERVICE_TOKEN", "ASSESSMENT_SERVICE_TOKEN")
	if legacyToken != "" {
		legacySubject := firstEnvironment("E2E_ASSESSMENT_LEGACY_SERVICE_SUBJECT", "ASSESSMENT_LEGACY_SERVICE_SUBJECT")
		if legacySubject == "" {
			legacySubject = "new-api"
		}
		if _, exists := configured[legacySubject]; !exists {
			configured[legacySubject] = configuredCredential{Token: legacyToken}
		}
	}

	result := make(map[string]serviceCredential, len(defaultCredentials))
	for role, subjects := range credentialSubjectCandidates {
		if lifecycleMode == lifecycleModeSkip && (role == "revoke" || role == "expiry") {
			continue
		}
		credential, subject, found := findConfiguredCredential(configured, subjects)
		if !found {
			return nil, fmt.Errorf("E2E assessment credential is missing for role %q (subjects: %s)", role, strings.Join(subjects, ", "))
		}
		token := strings.TrimSpace(credential.Token)
		if token == "" {
			for _, candidate := range credential.Tokens {
				if token = strings.TrimSpace(candidate); token != "" {
					break
				}
			}
		}
		if len(token) < 32 || len(token) > 4096 {
			return nil, fmt.Errorf("E2E assessment credential token for role %q must contain 32 to 4096 bytes", role)
		}
		result[role] = serviceCredential{subject: subject, token: token}
	}
	return result, nil
}

func findConfiguredCredential(configured map[string]configuredCredential, subjects []string) (configuredCredential, string, bool) {
	for _, subject := range subjects {
		if credential, exists := configured[subject]; exists {
			return credential, subject, true
		}
	}
	return configuredCredential{}, "", false
}

func cloneCredentials(source map[string]serviceCredential) map[string]serviceCredential {
	result := make(map[string]serviceCredential, len(source))
	for role, credential := range source {
		result[role] = credential
	}
	return result
}

type apiEnvelope struct {
	Success bool            `json:"success"`
	Data    json.RawMessage `json:"data"`
	Error   struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

func Run(ctx context.Context, config Config) (string, error) {
	identifier, err := randomID()
	if err != nil {
		return "", err
	}
	r := &runner{
		config: config, runID: time.Now().UTC().Format("20060102T150405") + "-" + identifier,
		phase:     "primary",
		startedAt: time.Now().UTC(),
		client:    &http.Client{Timeout: 2 * time.Minute, Transport: &http.Transport{Proxy: nil}},
	}
	return r.run(ctx)
}

func (r *runner) run(ctx context.Context) (string, error) {
	log.Printf("[%s] creating immutable GPUFabric technical inputs", r.runID)
	technical, err := r.createTechnicalInput(ctx)
	if err != nil {
		return "", err
	}
	log.Printf("[%s] creating T2 assessment and checking tenant isolation", r.runID)
	assessment, err := r.createAssessment(ctx, technical)
	if err != nil {
		return "", err
	}
	if err := r.expectServiceError(ctx, http.MethodGet, "/internal/v1/asset-assessments/"+assessment.AssessmentID, nil, r.config.Credentials["client"], "tenant-other", r.id("tenant-denial"), http.StatusNotFound, "ASSESSMENT_NOT_FOUND"); err != nil {
		return "", err
	}
	if err := r.validateEvidenceNegativeGates(ctx, assessment.AssessmentID); err != nil {
		return "", err
	}
	log.Printf("[%s] uploading, scanning, OCRing and reviewing required evidence", r.runID)
	for _, evidenceType := range []string{"ownership.invoice", "asset.lifecycle", "ownership.contract"} {
		if err := r.processEvidence(ctx, assessment.AssessmentID, evidenceType); err != nil {
			return "", err
		}
	}
	assessment, err = r.getAssessment(ctx, assessment.AssessmentID)
	if err != nil || assessment.Status != "ready_for_valuation" {
		return "", fmt.Errorf("evidence gate did not reach ready_for_valuation: %s: %w", assessment.Status, err)
	}
	log.Printf("[%s] validating market insufficiency, then ingesting automatic/manual samples", r.runID)
	market, err := r.createMarketSnapshot(ctx, assessment.AssetConfiguration)
	if err != nil {
		return "", err
	}
	log.Printf("[%s] authoring and independently approving pricing policy", r.runID)
	policy, err := r.createPricingPolicy(ctx, assessment.AssetConfiguration)
	if err != nil {
		return "", err
	}
	log.Printf("[%s] executing valuation and two-person formal review", r.runID)
	valuation, err := r.executeValuation(ctx, assessment.AssessmentID, technical.SnapshotID, market.SnapshotID, policy.Version)
	if err != nil {
		return "", err
	}
	if err := r.completeFormalReview(ctx, assessment.AssessmentID, valuation.ValuationID); err != nil {
		return "", err
	}
	log.Printf("[%s] freezing, rendering, HSM-signing, storing and downloading PDF", r.runID)
	report, pdf, err := r.issueAndDownload(ctx, assessment.AssessmentID)
	if err != nil {
		return "", err
	}
	result, err := r.archive(report, pdf, technical, market, valuation)
	if err != nil {
		return "", err
	}
	if r.config.LifecycleMode == lifecycleModeFull {
		log.Printf("[%s] validating revoked and expired report download denial", r.runID)
		r.phase = "revocation"
		revocationReport, err := r.createSecondaryReport(ctx, market, policy)
		if err != nil {
			return "", err
		}
		if err := r.revokeAndAssert(ctx, revocationReport.ReportID); err != nil {
			return "", err
		}
		r.phase = "expiry"
		if err := r.expireAndAssert(ctx, report); err != nil {
			return "", err
		}
		if r.config.CallbackMode == callbackModeLocal {
			if err := r.validateCallbackFlow(ctx, report.AssessmentID, revocationReport.AssessmentID); err != nil {
				return "", err
			}
		}
	} else {
		log.Printf("[%s] shared mode: report revocation, expiry and local callback sink gates skipped", r.runID)
	}
	return result, nil
}

type technicalInput struct {
	ReportID, ReportSHA256, ReportHTMLSHA256, SchemaVersion string
	SnapshotID, SnapshotSHA256, SnapshotSchemaVersion       string
	BenchmarkMetrics                                        []string
}

func (r *runner) createTechnicalInput(ctx context.Context) (technicalInput, error) {
	body := map[string]any{
		"gpufUserRef": r.config.GPUFabricUser, "gpufClientRef": r.config.GPUFabricClient,
		"tenantRef": r.config.TenantRef, "clientRequestId": r.id("gpuf-pre"), "assetName": "Local E2E GPU Node",
	}
	headers := map[string]string{
		"Authorization":   "Bearer " + r.config.GPUFabricToken,
		"Idempotency-Key": r.id("gpuf-pre"),
	}
	var created struct {
		ReportID, SchemaVersion string
		Benchmarks              []struct {
			Metric string `json:"metric"`
		} `json:"benchmarks"`
		TechnicalSnapshot struct {
			SnapshotID, SnapshotSHA256, SchemaVersion string
		} `json:"technicalSnapshot"`
	}
	if err := r.callEnvelope(ctx, http.MethodPost, r.config.GPUFabricURL+"/internal/v1/technical-pre-evaluations/from-client", body, headers, http.StatusOK, &created); err != nil {
		return technicalInput{}, err
	}
	var integrity struct {
		ReportSHA256, ReportHTMLSHA256, HashProfile, HTMLHashProfile string
	}
	if err := r.callEnvelope(ctx, http.MethodGet, r.config.GPUFabricURL+"/internal/v1/technical-pre-evaluations/"+url.PathEscape(created.ReportID), nil, headers, http.StatusOK, &integrity); err != nil {
		return technicalInput{}, err
	}
	if created.ReportID == "" || created.TechnicalSnapshot.SnapshotID == "" || integrity.HashProfile != "gpuf.report-json-bytes.v1" || integrity.HTMLHashProfile != "gpuf.report-html-bytes.v1" {
		return technicalInput{}, errors.New("GPUFabric technical integrity envelope is incomplete")
	}
	metrics := make([]string, 0, len(created.Benchmarks))
	for _, value := range created.Benchmarks {
		metrics = append(metrics, value.Metric)
	}
	if !contains(metrics, "tokens_per_second") || !(contains(metrics, "stability_pass_rate") || contains(metrics, "sustained_throughput_percent")) {
		return technicalInput{}, errors.New("GPUFabric T2 benchmark categories are incomplete")
	}
	return technicalInput{
		ReportID: created.ReportID, ReportSHA256: integrity.ReportSHA256,
		ReportHTMLSHA256: integrity.ReportHTMLSHA256, SchemaVersion: created.SchemaVersion,
		SnapshotID:            created.TechnicalSnapshot.SnapshotID,
		SnapshotSHA256:        created.TechnicalSnapshot.SnapshotSHA256,
		SnapshotSchemaVersion: created.TechnicalSnapshot.SchemaVersion,
		BenchmarkMetrics:      metrics,
	}, nil
}

type assetConfiguration struct {
	CanonicalModelID  string `json:"canonicalModelId"`
	ConfigurationHash string `json:"configurationHash"`
	DeviceForm        string `json:"deviceForm"`
	GPUCount          int    `json:"gpuCount"`
	MemoryPerGPUBytes int64  `json:"memoryPerGpuBytes"`
}

type assessmentView struct {
	AssessmentID       string `json:"assessmentId"`
	Status             string `json:"status"`
	AssetConfiguration assetConfiguration
}

func (r *runner) createAssessment(ctx context.Context, input technicalInput) (assessmentView, error) {
	body := map[string]any{
		"clientRequestId": r.id("assessment"), "correlationId": r.id("correlation"),
		"tenantRef": r.config.TenantRef, "userRef": r.config.GPUFabricUser,
		"assetRef":      "gpufabric:client:" + r.config.GPUFabricClient,
		"requestedTier": "T2", "purpose": []string{"financing_pre_review"},
		"preEvaluation": map[string]any{
			"provider": "gpufabric", "reportId": input.ReportID, "reportSha256": input.ReportSHA256,
			"reportHtmlSha256": input.ReportHTMLSHA256, "schemaVersion": input.SchemaVersion,
			"technicalSnapshotId": input.SnapshotID, "technicalSnapshotSha256": input.SnapshotSHA256,
			"technicalSnapshotSchemaVersion": input.SnapshotSchemaVersion,
		},
		"callback": map[string]string{"urlRef": ""},
	}
	var response struct {
		AssessmentID          string `json:"assessmentId"`
		Status                string `json:"status"`
		TechnicalVerification struct {
			Status             string             `json:"status"`
			AssetConfiguration assetConfiguration `json:"assetConfiguration"`
			BenchmarkPolicy    struct {
				Status string `json:"status"`
			} `json:"benchmarkPolicy"`
		} `json:"technicalVerification"`
	}
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments", body, r.config.Credentials["client"], r.config.TenantRef, r.id("assessment"), http.StatusAccepted, &response); err != nil {
		return assessmentView{}, err
	}
	if response.Status != "evidence_pending" || response.TechnicalVerification.Status != "verified" || response.TechnicalVerification.BenchmarkPolicy.Status != "satisfied" {
		return assessmentView{}, errors.New("T2 technical verification gate was not satisfied")
	}
	return assessmentView{AssessmentID: response.AssessmentID, Status: response.Status, AssetConfiguration: response.TechnicalVerification.AssetConfiguration}, nil
}

func (r *runner) getAssessment(ctx context.Context, assessmentID string) (assessmentView, error) {
	var response struct {
		AssessmentID          string `json:"assessmentId"`
		Status                string `json:"status"`
		TechnicalVerification struct {
			AssetConfiguration assetConfiguration `json:"assetConfiguration"`
		} `json:"technicalVerification"`
	}
	err := r.callService(ctx, http.MethodGet, "/internal/v1/asset-assessments/"+url.PathEscape(assessmentID), nil, r.config.Credentials["client"], r.config.TenantRef, r.id("get-assessment"), http.StatusOK, &response)
	return assessmentView{AssessmentID: response.AssessmentID, Status: response.Status, AssetConfiguration: response.TechnicalVerification.AssetConfiguration}, err
}

type uploadGrant struct {
	EvidenceID      string            `json:"evidenceId"`
	UploadMethod    string            `json:"uploadMethod"`
	UploadURL       string            `json:"uploadUrl"`
	RequiredHeaders map[string]string `json:"requiredHeaders"`
}

type accessGrant struct {
	DownloadMethod  string            `json:"downloadMethod"`
	DownloadURL     string            `json:"downloadUrl"`
	RequiredHeaders map[string]string `json:"requiredHeaders"`
	ContentType     string            `json:"contentType"`
	ContentLength   int64             `json:"contentLength"`
	SHA256          string            `json:"sha256"`
}

type scanResult struct {
	Status, DetectedContentType, SHA256, ReasonCode, OCRText, OCRSHA256 string
}

type evidenceResult struct {
	Status, VerificationCode string
}

func (r *runner) validateEvidenceNegativeGates(ctx context.Context, assessmentID string) error {
	pdf, digest, err := r.renderEvidence(ctx, "ownership.invoice.negative")
	if err != nil {
		return err
	}
	requestID := r.id("evidence-hash-mismatch")
	grant, err := r.createEvidenceUpload(ctx, assessmentID, requestID, "ownership.invoice", "hash-mismatch.pdf", pdf, digest)
	if err != nil {
		return err
	}
	var mismatch evidenceResult
	completionID := requestID + "-complete"
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/upload-completions", map[string]any{
		"eventId": completionID, "contentLength": len(pdf), "sha256": strings.Repeat("0", sha256.Size*2),
	}, r.config.Credentials["storage"], r.config.TenantRef, completionID, http.StatusOK, &mismatch); err != nil {
		return err
	}
	if mismatch.Status != "rejected" || mismatch.VerificationCode != "EVIDENCE_HASH_MISMATCH" {
		return fmt.Errorf("hash mismatch evidence was not rejected: %+v", mismatch)
	}

	eicar := []byte("X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*")
	eicarDigest := sha256Hex(eicar)
	requestID = r.id("evidence-eicar")
	grant, err = r.createEvidenceUpload(ctx, assessmentID, requestID, "ownership.invoice", "eicar-test.pdf", eicar, eicarDigest)
	if err != nil {
		return err
	}
	completionID = requestID + "-complete"
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/upload-completions", map[string]any{
		"eventId": completionID, "contentLength": len(eicar), "sha256": eicarDigest,
	}, r.config.Credentials["storage"], r.config.TenantRef, completionID, http.StatusOK, nil); err != nil {
		return err
	}
	accessID := requestID + "-scan-access"
	var access accessGrant
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/scan-downloads", map[string]string{"clientRequestId": accessID}, r.config.Credentials["scanner"], r.config.TenantRef, accessID, http.StatusCreated, &access); err != nil {
		return err
	}
	var scanned scanResult
	if err := r.callEnvelope(ctx, http.MethodPost, r.config.SupportURL+"/internal/v1/evidence-scans", map[string]any{
		"jobId": requestID + "-scan", "downloadMethod": access.DownloadMethod,
		"downloadUrl": access.DownloadURL, "requiredHeaders": access.RequiredHeaders,
		"expectedSha256": access.SHA256, "expectedContentType": access.ContentType,
		"expectedContentLength": access.ContentLength, "fileName": "eicar-test.pdf",
	}, map[string]string{"Authorization": "Bearer " + r.config.SupportToken}, http.StatusOK, &scanned); err != nil {
		return err
	}
	if scanned.Status != "infected" || scanned.ReasonCode != "MALWARE_DETECTED" || scanned.SHA256 != eicarDigest {
		return fmt.Errorf("EICAR fixture was not detected: %+v", scanned)
	}
	scanID := requestID + "-scan-result"
	var infected evidenceResult
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/scan-results", map[string]any{
		"eventId": scanID, "status": scanned.Status, "detectedContentType": scanned.DetectedContentType,
		"sha256": scanned.SHA256, "reasonCode": scanned.ReasonCode,
	}, r.config.Credentials["scanner"], r.config.TenantRef, scanID, http.StatusOK, &infected); err != nil {
		return err
	}
	if infected.Status != "rejected" || infected.VerificationCode != "MALWARE_DETECTED" {
		return fmt.Errorf("infected evidence was not rejected: %+v", infected)
	}
	return r.expectServiceError(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/scan-results", map[string]any{
		"eventId": scanID, "status": "clean", "detectedContentType": "application/pdf", "sha256": scanned.SHA256,
	}, r.config.Credentials["scanner"], r.config.TenantRef, scanID, http.StatusConflict, "IDEMPOTENCY_CONFLICT")
}

func (r *runner) createEvidenceUpload(ctx context.Context, assessmentID, requestID, evidenceType, fileName string, content []byte, digest string) (uploadGrant, error) {
	var grant uploadGrant
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence-sessions", map[string]any{
		"clientRequestId": requestID, "evidenceType": evidenceType, "contentType": "application/pdf",
		"contentLength": len(content), "fileName": fileName, "sha256": digest,
	}, r.config.Credentials["client"], r.config.TenantRef, requestID, http.StatusCreated, &grant); err != nil {
		return uploadGrant{}, err
	}
	if err := r.putBytes(ctx, grant.UploadMethod, grant.UploadURL, grant.RequiredHeaders, content); err != nil {
		return uploadGrant{}, err
	}
	return grant, nil
}

func (r *runner) processEvidence(ctx context.Context, assessmentID, evidenceType string) error {
	pdf, digest, err := r.renderEvidence(ctx, evidenceType)
	if err != nil {
		return err
	}
	requestID := r.id(strings.ReplaceAll(evidenceType, ".", "-"))
	var grant uploadGrant
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence-sessions", map[string]any{
		"clientRequestId": requestID, "evidenceType": evidenceType, "contentType": "application/pdf",
		"contentLength": len(pdf), "fileName": strings.ReplaceAll(evidenceType, ".", "-") + ".pdf", "sha256": digest,
	}, r.config.Credentials["client"], r.config.TenantRef, requestID, http.StatusCreated, &grant); err != nil {
		return err
	}
	if err := r.putBytes(ctx, grant.UploadMethod, grant.UploadURL, grant.RequiredHeaders, pdf); err != nil {
		return err
	}
	completionID := requestID + "-upload"
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/upload-completions", map[string]any{
		"eventId": completionID, "contentLength": len(pdf), "sha256": digest,
	}, r.config.Credentials["storage"], r.config.TenantRef, completionID, http.StatusOK, nil); err != nil {
		return err
	}
	scanAccessID := requestID + "-scan-access"
	var scanAccess accessGrant
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/scan-downloads", map[string]string{"clientRequestId": scanAccessID}, r.config.Credentials["scanner"], r.config.TenantRef, scanAccessID, http.StatusCreated, &scanAccess); err != nil {
		return err
	}
	var scanned scanResult
	if err := r.callEnvelope(ctx, http.MethodPost, r.config.SupportURL+"/internal/v1/evidence-scans", map[string]any{
		"jobId": requestID + "-scan", "downloadMethod": scanAccess.DownloadMethod,
		"downloadUrl": scanAccess.DownloadURL, "requiredHeaders": scanAccess.RequiredHeaders,
		"expectedSha256": scanAccess.SHA256, "expectedContentType": scanAccess.ContentType,
		"expectedContentLength": scanAccess.ContentLength, "fileName": evidenceType + ".pdf",
	}, map[string]string{"Authorization": "Bearer " + r.config.SupportToken}, http.StatusOK, &scanned); err != nil {
		return err
	}
	if scanned.Status != "clean" || scanned.SHA256 != digest || scanned.DetectedContentType != "application/pdf" || scanned.OCRSHA256 == "" || scanned.OCRText == "" {
		return fmt.Errorf("evidence scanner did not produce clean OCR output for %s", evidenceType)
	}
	scanID := requestID + "-scan-result"
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/scan-results", map[string]any{
		"eventId": scanID, "status": scanned.Status, "detectedContentType": scanned.DetectedContentType, "sha256": scanned.SHA256,
	}, r.config.Credentials["scanner"], r.config.TenantRef, scanID, http.StatusOK, nil); err != nil {
		return err
	}
	reviewAccessID := requestID + "-review-access"
	var reviewAccess accessGrant
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/review-downloads", map[string]string{"clientRequestId": reviewAccessID}, r.config.Credentials["evidence"], r.config.TenantRef, reviewAccessID, http.StatusCreated, &reviewAccess); err != nil {
		return err
	}
	reviewBytes, err := r.getBytes(ctx, reviewAccess.DownloadURL, reviewAccess.RequiredHeaders)
	if err != nil || sha256Hex(reviewBytes) != digest {
		return fmt.Errorf("review download integrity failed for %s: %w", evidenceType, err)
	}
	reviewID := requestID + "-review"
	reviewBody := map[string]any{"clientRequestId": reviewID, "action": "verify", "reasonCode": "LOCAL_E2E_VERIFIED"}
	if evidenceType == "asset.lifecycle" {
		reviewBody["lifecycleFacts"] = map[string]any{
			"condition": "good", "manufacturedAt": "2024-01-01T00:00:00Z",
			"commissionedAt": "2024-02-01T00:00:00Z", "warrantyUntil": time.Now().UTC().AddDate(1, 0, 0),
		}
	}
	return r.callServiceWithReviewer(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/evidence/"+grant.EvidenceID+"/review-actions", reviewBody, r.config.Credentials["evidence"], r.config.TenantRef, reviewID, "evidence-reviewer-local", http.StatusOK, nil)
}

func (r *runner) renderEvidence(ctx context.Context, evidenceType string) ([]byte, string, error) {
	html := "<!doctype html><html><head><meta charset=\"utf-8\"></head><body><h1>Local E2E Evidence</h1><p>Type: " + evidenceType + "</p><p>Run: " + r.runID + "</p><p>Verified fixture content for OCR.</p></body></html>"
	var response struct{ PDFBase64, PDFSHA256 string }
	if err := r.callEnvelope(ctx, http.MethodPost, r.config.SupportURL+"/internal/v1/pdf-renders", map[string]string{
		"reportId": r.id("evidence-" + strings.ReplaceAll(evidenceType, ".", "-")), "html": html, "htmlSha256": sha256Hex([]byte(html)),
	}, map[string]string{"Authorization": "Bearer " + r.config.SupportToken}, http.StatusOK, &response); err != nil {
		return nil, "", err
	}
	pdf, err := base64.StdEncoding.DecodeString(response.PDFBase64)
	if err != nil || !bytes.HasPrefix(pdf, []byte("%PDF-")) || sha256Hex(pdf) != response.PDFSHA256 {
		return nil, "", errors.New("support evidence PDF integrity failed")
	}
	return pdf, response.PDFSHA256, nil
}

type marketResult struct{ SnapshotID, SnapshotSHA256 string }

type marketFixture struct {
	SourceMode, SourceProvider, SourceRecordID, Currency, Condition string
	TransactionPriceMinor                                           int64
}

func (r *runner) createMarketSnapshot(ctx context.Context, configuration assetConfiguration) (marketResult, error) {
	windowStart := r.startedAt.Add(-2 * time.Second)
	windowEnd := time.Now().UTC().Add(time.Second)
	snapshotBody := func(id string) map[string]any {
		return map[string]any{
			"clientRequestId": id, "assetConfiguration": configuration, "condition": "good",
			"region": "US", "currency": "USD", "windowStart": windowStart,
			"windowEnd": windowEnd, "observationCutoffAt": windowEnd.Add(time.Second),
			"aggregationPolicyVersion": "market_aggregation.v1",
		}
	}
	if err := r.expectServiceError(ctx, http.MethodPost, "/internal/v1/market-price-snapshots", snapshotBody(r.id("market-insufficient")), r.config.Credentials["snapshot"], r.config.TenantRef, r.id("market-insufficient"), http.StatusUnprocessableEntity, "MARKET_DATA_INSUFFICIENT"); err != nil {
		return marketResult{}, err
	}
	entries, err := os.ReadDir(r.config.FixtureDir)
	if err != nil {
		return marketResult{}, err
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Name() < entries[j].Name() })
	if len(entries) != 3 {
		return marketResult{}, fmt.Errorf("market fixture count = %d, want 3", len(entries))
	}
	for index, entry := range entries {
		raw, err := os.ReadFile(filepath.Join(r.config.FixtureDir, entry.Name()))
		if err != nil {
			return marketResult{}, err
		}
		var fixture marketFixture
		if err := json.Unmarshal(raw, &fixture); err != nil {
			return marketResult{}, err
		}
		provider := r.config.Credentials["market-a"]
		if index == 1 {
			provider = r.config.Credentials["market-b"]
		}
		requestID := r.id("market-" + strconv.Itoa(index+1))
		price := fixture.TransactionPriceMinor
		var observation struct {
			ObservationID string `json:"observationId"`
		}
		if err := r.callService(ctx, http.MethodPost, "/internal/v1/market-observations", map[string]any{
			"clientRequestId": requestID, "licensePolicyId": "local-e2e-synthetic-v1", "sourceType": "closed_deal",
			"capturedAt": r.startedAt.Add(-time.Second + time.Duration(index)*200*time.Millisecond),
			"region":     "US", "currency": "USD", "assetConfiguration": configuration, "condition": "good",
			"transactionPriceMinor": price, "sourceRecordHash": sha256Hex([]byte(fixture.SourceRecordID)),
			"evidenceSha256": sha256Hex(raw), "rawObjectRef": "s3://private-market-evidence/market/" + entry.Name(),
			"retentionUntil": time.Now().UTC().Add(30 * 24 * time.Hour),
		}, provider, r.config.TenantRef, requestID, http.StatusCreated, &observation); err != nil {
			return marketResult{}, err
		}
		verifyID := requestID + "-verify"
		if err := r.callService(ctx, http.MethodPost, "/internal/v1/market-observations/"+observation.ObservationID+"/verification-actions", map[string]any{
			"eventId": verifyID, "action": "verify", "qualityScore": 85,
		}, r.config.Credentials["market-verify"], r.config.TenantRef, verifyID, http.StatusOK, nil); err != nil {
			return marketResult{}, err
		}
	}
	finalID := r.id("market-snapshot")
	var snapshot struct {
		MarketSnapshotID, SnapshotSHA256            string
		SampleCount, ProviderCount, ClosedDealCount int
		Confidence                                  float64
	}
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/market-price-snapshots", snapshotBody(finalID), r.config.Credentials["snapshot"], r.config.TenantRef, finalID, http.StatusCreated, &snapshot); err != nil {
		return marketResult{}, err
	}
	if snapshot.SampleCount < 3 || snapshot.ProviderCount < 2 || snapshot.ClosedDealCount < 3 || snapshot.Confidence < 0.5 {
		return marketResult{}, errors.New("market snapshot evidence gate is incomplete")
	}
	return marketResult{SnapshotID: snapshot.MarketSnapshotID, SnapshotSHA256: snapshot.SnapshotSHA256}, nil
}

type policyResult struct{ ID, Version string }

func (r *runner) createPricingPolicy(ctx context.Context, configuration assetConfiguration) (policyResult, error) {
	version := "local-e2e-pricing-" + r.runID
	var policy struct{ PolicyID, PolicyVersion, Status string }
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/pricing-policies", map[string]any{
		"policyVersion": version, "effectiveFrom": time.Now().UTC().Add(-time.Hour),
		"supportedRegions": []string{"US"}, "supportedAssetClasses": []string{configuration.DeviceForm},
		"algorithmDigest":          sha256Hex([]byte("local-e2e-comparable-v1")),
		"marketAggregationVersion": "market_aggregation.v1", "depreciationCurveVersion": "depreciation.local-e2e.v1",
		"conditionAdjustments": map[string]float64{"good": 0.95},
		"warrantyAdjustments":  map[string]float64{"covered": 1, "default": 0.9},
		"liquidityAdjustments": map[string]float64{"US": 1}, "minimumConfidence": 0.5,
	}, r.config.Credentials["policy-author"], r.config.TenantRef, r.id("policy-create"), http.StatusCreated, &policy); err != nil {
		return policyResult{}, err
	}
	approvalID := r.id("policy-approve")
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/pricing-policies/"+policy.PolicyID+"/approval-actions", map[string]string{
		"eventId": approvalID, "approverRef": "pricing-approver-local-e2e",
	}, r.config.Credentials["policy-approve"], r.config.TenantRef, approvalID, http.StatusOK, &policy); err != nil {
		return policyResult{}, err
	}
	if policy.Status != "approved" {
		return policyResult{}, errors.New("pricing policy was not approved")
	}
	return policyResult{ID: policy.PolicyID, Version: policy.PolicyVersion}, nil
}

type valuationResult struct{ ValuationID, ValuationSHA256 string }

func (r *runner) executeValuation(ctx context.Context, assessmentID, snapshotID, marketID, policyVersion string) (valuationResult, error) {
	var result struct {
		ValuationID, ValuationSHA256, Currency string
		PointValueMinor                        *int64
		LowValueMinor, HighValueMinor          int64
		Confidence                             float64
	}
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/valuation", map[string]string{
		"technicalSnapshotId": snapshotID, "marketSnapshotId": marketID, "policyVersion": policyVersion, "method": "comparable",
	}, r.config.Credentials["valuation"], r.config.TenantRef, r.id("valuation"), http.StatusCreated, &result); err != nil {
		return valuationResult{}, err
	}
	if result.PointValueMinor == nil || result.LowValueMinor <= 0 || result.HighValueMinor < result.LowValueMinor || result.Currency != "USD" || result.Confidence < 0.5 {
		return valuationResult{}, errors.New("valuation result failed objective range checks")
	}
	return valuationResult{ValuationID: result.ValuationID, ValuationSHA256: result.ValuationSHA256}, nil
}

func (r *runner) completeFormalReview(ctx context.Context, assessmentID, valuationID string) error {
	submitID := r.id("review-submit")
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/submit-review", map[string]string{
		"clientRequestId": submitID, "valuationId": valuationID,
	}, r.config.Credentials["valuation"], r.config.TenantRef, submitID, http.StatusCreated, nil); err != nil {
		return err
	}
	assignments := []struct{ role, reviewer, suffix string }{
		{"primary", "reviewer-primary-local", "assign-primary"},
		{"secondary", "reviewer-secondary-local", "assign-secondary"},
	}
	for index, assignment := range assignments {
		requestID := r.id(assignment.suffix)
		if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/review-assignments", map[string]string{
			"clientRequestId": requestID, "role": assignment.role, "reviewerRef": assignment.reviewer,
		}, r.config.Credentials["coordinator"], r.config.TenantRef, requestID, http.StatusOK, nil); err != nil {
			return err
		}
		if index == 0 {
			separationID := r.id("assign-secondary-same-reviewer")
			if err := r.expectServiceError(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/review-assignments", map[string]string{
				"clientRequestId": separationID, "role": "secondary", "reviewerRef": assignment.reviewer,
			}, r.config.Credentials["coordinator"], r.config.TenantRef, separationID, http.StatusConflict, "REVIEWER_SEPARATION_REQUIRED"); err != nil {
				return err
			}
		}
	}
	for _, action := range []struct{ reviewer, action, reason, suffix string }{
		{"reviewer-primary-local", "start_review", "REVIEW_STARTED", "review-start"},
		{"reviewer-primary-local", "approve", "PRIMARY_APPROVED", "review-primary-approve"},
		{"reviewer-secondary-local", "approve", "SECONDARY_APPROVED", "review-secondary-approve"},
	} {
		requestID := r.id(action.suffix)
		var view struct {
			Review struct {
				Status string `json:"status"`
			} `json:"review"`
		}
		if err := r.callServiceWithReviewer(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/review-actions", map[string]string{
			"clientRequestId": requestID, "action": action.action, "reasonCode": action.reason,
		}, r.config.Credentials["workbench"], r.config.TenantRef, requestID, action.reviewer, http.StatusOK, &view); err != nil {
			return err
		}
		if action.suffix == "review-secondary-approve" && view.Review.Status != "approved" {
			return errors.New("formal review did not reach approved")
		}
	}
	return nil
}

func (r *runner) createSecondaryReport(ctx context.Context, market marketResult, policy policyResult) (reportResult, error) {
	technical, err := r.createTechnicalInput(ctx)
	if err != nil {
		return reportResult{}, err
	}
	assessment, err := r.createAssessment(ctx, technical)
	if err != nil {
		return reportResult{}, err
	}
	for _, evidenceType := range []string{"ownership.invoice", "asset.lifecycle", "ownership.contract"} {
		if err := r.processEvidence(ctx, assessment.AssessmentID, evidenceType); err != nil {
			return reportResult{}, err
		}
	}
	assessment, err = r.getAssessment(ctx, assessment.AssessmentID)
	if err != nil || assessment.Status != "ready_for_valuation" {
		return reportResult{}, fmt.Errorf("secondary evidence gate failed: %s: %w", assessment.Status, err)
	}
	valuation, err := r.executeValuation(ctx, assessment.AssessmentID, technical.SnapshotID, market.SnapshotID, policy.Version)
	if err != nil {
		return reportResult{}, err
	}
	if err := r.completeFormalReview(ctx, assessment.AssessmentID, valuation.ValuationID); err != nil {
		return reportResult{}, err
	}
	report, _, err := r.issueAndDownload(ctx, assessment.AssessmentID)
	return report, err
}

func (r *runner) revokeAndAssert(ctx context.Context, reportID string) error {
	requestID := r.id("report-revoke")
	var report reportResult
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/reports/"+reportID+"/revoke", map[string]string{
		"clientRequestId": requestID, "reasonCode": "LOCAL_E2E_REVOCATION",
	}, r.config.Credentials["revoke"], r.config.TenantRef, requestID, http.StatusOK, &report); err != nil {
		return err
	}
	if report.Status != "revoked" {
		return fmt.Errorf("report did not reach revoked: %s", report.Status)
	}
	downloadID := r.id("download-after-revoke")
	return r.expectServiceError(ctx, http.MethodPost, "/internal/v1/reports/"+reportID+"/downloads", map[string]string{
		"clientRequestId": downloadID,
	}, r.config.Credentials["download"], r.config.TenantRef, downloadID, http.StatusGone, "REPORT_DOWNLOAD_DENIED")
}

func (r *runner) expireAndAssert(ctx context.Context, report reportResult) error {
	if report.ValidUntil == nil {
		return errors.New("issued report has no validity deadline")
	}
	wait := time.Until(report.ValidUntil.Add(500 * time.Millisecond))
	if wait > 30*time.Second {
		return fmt.Errorf("local report validity %s is too long for the expiry E2E gate", wait)
	}
	if wait > 0 {
		timer := time.NewTimer(wait)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
		}
	}
	requestID := r.id("report-expiry-sweep")
	var result struct {
		ExpiredCount int `json:"expiredCount"`
	}
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/report-expirations", map[string]int{"limit": 100}, r.config.Credentials["expiry"], r.config.TenantRef, requestID, http.StatusOK, &result); err != nil {
		return err
	}
	if result.ExpiredCount < 1 {
		return errors.New("report expiry sweep did not expire the issued report")
	}
	downloadID := r.id("download-after-expiry")
	return r.expectServiceError(ctx, http.MethodPost, "/internal/v1/reports/"+report.ReportID+"/downloads", map[string]string{
		"clientRequestId": downloadID,
	}, r.config.Credentials["download"], r.config.TenantRef, downloadID, http.StatusGone, "REPORT_DOWNLOAD_DENIED")
}

type callbackStats struct {
	AcceptedCount int                 `json:"acceptedCount"`
	ReplayCount   int                 `json:"replayCount"`
	ConflictCount int                 `json:"conflictCount"`
	Assessments   map[string][]string `json:"assessments"`
}

func (r *runner) validateCallbackFlow(ctx context.Context, expiredAssessmentID, revokedAssessmentID string) error {
	deadline := time.Now().Add(12 * time.Second)
	for {
		var stats callbackStats
		if err := r.callEnvelope(ctx, http.MethodGet, r.config.SupportURL+"/internal/v1/callback-state", nil, map[string]string{
			"Authorization": "Bearer " + r.config.SupportToken,
		}, http.StatusOK, &stats); err != nil {
			return err
		}
		if contains(stats.Assessments[expiredAssessmentID], "expired") && contains(stats.Assessments[revokedAssessmentID], "revoked") {
			break
		}
		if time.Now().After(deadline) {
			return fmt.Errorf("outbox callback terminal states were not delivered: expired=%v revoked=%v", stats.Assessments[expiredAssessmentID], stats.Assessments[revokedAssessmentID])
		}
		timer := time.NewTimer(200 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return ctx.Err()
		case <-timer.C:
		}
	}

	eventID := r.id("callback-replay")
	timestamp := strconv.FormatInt(time.Now().UTC().Unix(), 10)
	body := map[string]any{
		"eventId": eventID, "eventType": "asset_assessment.status_changed",
		"schemaVersion": "asset_assessment_event.v1", "occurredAt": time.Now().UTC(),
		"correlationId": r.id("callback-correlation"), "assessmentId": "ASMT-CALLBACK-" + r.runID,
		"clientRequestId": r.id("callback-client"), "assetRef": "asset-callback-local-e2e",
		"status": "evidence_pending", "progress": 30, "requiredEvidenceCodes": []string{},
		"report": nil, "error": nil,
	}
	encoded, err := json.Marshal(body)
	if err != nil {
		return err
	}
	signature := callbackSignature(r.config.CallbackSecret, eventID, timestamp, encoded)
	first, err := r.callCallback(ctx, eventID, timestamp, signature, encoded, http.StatusOK)
	if err != nil || first {
		return fmt.Errorf("first callback was not accepted as new: duplicate=%v: %w", first, err)
	}
	duplicate, err := r.callCallback(ctx, eventID, timestamp, signature, encoded, http.StatusOK)
	if err != nil || !duplicate {
		return fmt.Errorf("callback replay was not accepted idempotently: duplicate=%v: %w", duplicate, err)
	}
	body["status"] = "reviewing"
	conflicting, _ := json.Marshal(body)
	if _, err := r.callCallback(ctx, eventID, timestamp, callbackSignature(r.config.CallbackSecret, eventID, timestamp, conflicting), conflicting, http.StatusConflict); err != nil {
		return err
	}
	if _, err := r.callCallback(ctx, eventID, timestamp, signature, []byte(`{"tampered":true}`), http.StatusForbidden); err != nil {
		return err
	}
	return nil
}

func (r *runner) callCallback(ctx context.Context, eventID, timestamp, signature string, body []byte, expectedStatus int) (bool, error) {
	status, response, err := r.request(ctx, http.MethodPost, r.config.SupportURL+"/api/banking/callback/assessment", json.RawMessage(body), map[string]string{
		"X-Event-ID": eventID, "X-Event-Timestamp": timestamp, "X-Event-Signature": signature,
	})
	if err != nil {
		return false, err
	}
	var envelope struct {
		Success bool   `json:"success"`
		Code    string `json:"code"`
		Data    struct {
			Duplicate bool `json:"duplicate"`
		} `json:"data"`
	}
	if err := json.Unmarshal(response, &envelope); err != nil {
		return false, err
	}
	if status != expectedStatus {
		return false, fmt.Errorf("callback returned HTTP %d, want %d: %s", status, expectedStatus, strings.TrimSpace(string(response)))
	}
	if expectedStatus == http.StatusOK && (!envelope.Success || envelope.Code != "EVENT_ACCEPTED") {
		return false, errors.New("callback success envelope is invalid")
	}
	return envelope.Data.Duplicate, nil
}

func callbackSignature(secret, eventID, timestamp string, body []byte) string {
	digest := sha256.Sum256(body)
	canonical := http.MethodPost + "\n/api/banking/callback/assessment\n" + eventID + "\n" + timestamp + "\n" + hex.EncodeToString(digest[:])
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	return "v1=" + hex.EncodeToString(mac.Sum(nil))
}

type reportSignature struct {
	SchemaVersion, Algorithm, KeyVersion, CertificateChainSHA256 string
	SigningDigestSHA256, Signature, TimestampAuthority           string
	TimestampTokenSHA256, TimestampToken                         string
	CertificateChain                                             []string  `json:"certificateChain"`
	SignedAt                                                     time.Time `json:"signedAt"`
}

type reportResult struct {
	ReportID, AssessmentID, ReportJSONSHA256, ReportHTMLSHA256, ReportPDFSHA256 string
	ReportVersion                                                               int
	Status                                                                      string `json:"reportStatus"`
	FrozenAt                                                                    time.Time
	IssuedAt, ValidUntil                                                        *time.Time
	Signature                                                                   *reportSignature
}

func (r *runner) issueAndDownload(ctx context.Context, assessmentID string) (reportResult, []byte, error) {
	freezeID := r.id("report-freeze")
	var report reportResult
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/asset-assessments/"+assessmentID+"/report-freezes", map[string]string{"clientRequestId": freezeID}, r.config.Credentials["freeze"], r.config.TenantRef, freezeID, http.StatusCreated, &report); err != nil {
		return reportResult{}, nil, err
	}
	issueID := r.id("report-issue")
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/reports/"+report.ReportID+"/issue", map[string]string{"clientRequestId": issueID}, r.config.Credentials["issue"], r.config.TenantRef, issueID, http.StatusOK, &report); err != nil {
		return reportResult{}, nil, err
	}
	if report.Status != "issued" || report.Signature == nil || report.IssuedAt == nil || report.ValidUntil == nil {
		return reportResult{}, nil, errors.New("issued report lifecycle fields are incomplete")
	}
	if err := verifyReportSignature(report); err != nil {
		return reportResult{}, nil, err
	}
	downloadID := r.id("report-download")
	var grant struct{ URL, Method string }
	if err := r.callService(ctx, http.MethodPost, "/internal/v1/reports/"+report.ReportID+"/downloads", map[string]string{"clientRequestId": downloadID}, r.config.Credentials["download"], r.config.TenantRef, downloadID, http.StatusCreated, &grant); err != nil {
		return reportResult{}, nil, err
	}
	pdf, err := r.getBytes(ctx, grant.URL, nil)
	if err != nil {
		return reportResult{}, nil, err
	}
	if grant.Method != http.MethodGet || !bytes.HasPrefix(pdf, []byte("%PDF-")) || sha256Hex(pdf) != report.ReportPDFSHA256 {
		return reportResult{}, nil, errors.New("downloaded report PDF integrity failed")
	}
	return report, pdf, nil
}

type signingEnvelope struct {
	SchemaVersion    string    `json:"schemaVersion"`
	ReportID         string    `json:"reportId"`
	ReportVersion    int       `json:"reportVersion"`
	ReportJSONSHA256 string    `json:"reportJsonSha256"`
	ReportHTMLSHA256 string    `json:"reportHtmlSha256"`
	ReportPDFSHA256  string    `json:"reportPdfSha256"`
	FrozenAt         time.Time `json:"frozenAt"`
	IssuedAt         time.Time `json:"issuedAt"`
	ValidUntil       time.Time `json:"validUntil"`
}

func verifyReportSignature(report reportResult) error {
	signature := report.Signature
	encoded, err := json.Marshal(signingEnvelope{
		SchemaVersion: "asset_assessment.report-signing.v1", ReportID: report.ReportID,
		ReportVersion: report.ReportVersion, ReportJSONSHA256: report.ReportJSONSHA256,
		ReportHTMLSHA256: report.ReportHTMLSHA256, ReportPDFSHA256: report.ReportPDFSHA256,
		FrozenAt: report.FrozenAt, IssuedAt: *report.IssuedAt, ValidUntil: *report.ValidUntil,
	})
	if err != nil || sha256Hex(encoded) != signature.SigningDigestSHA256 {
		return errors.New("report signing envelope digest mismatch")
	}
	if signature.SchemaVersion != "asset_assessment.report-signature.v1" || signature.Algorithm != "ECDSA-P256-SHA256" || len(signature.CertificateChain) < 2 {
		return errors.New("unsupported report signature metadata")
	}
	certificates := make([]*x509.Certificate, 0, len(signature.CertificateChain))
	for _, encodedCertificate := range signature.CertificateChain {
		block, _ := pem.Decode([]byte(encodedCertificate))
		if block == nil || block.Type != "CERTIFICATE" {
			return errors.New("report certificate chain is invalid")
		}
		certificate, err := x509.ParseCertificate(block.Bytes)
		if err != nil {
			return err
		}
		certificates = append(certificates, certificate)
	}
	roots := x509.NewCertPool()
	roots.AddCert(certificates[len(certificates)-1])
	intermediates := x509.NewCertPool()
	for _, certificate := range certificates[1 : len(certificates)-1] {
		intermediates.AddCert(certificate)
	}
	if _, err := certificates[0].Verify(x509.VerifyOptions{Roots: roots, Intermediates: intermediates, CurrentTime: *report.IssuedAt, KeyUsages: []x509.ExtKeyUsage{x509.ExtKeyUsageAny}}); err != nil {
		return err
	}
	publicKey, ok := certificates[0].PublicKey.(*ecdsa.PublicKey)
	if !ok {
		return errors.New("report leaf key is not ECDSA")
	}
	digest, err := hex.DecodeString(signature.SigningDigestSHA256)
	if err != nil {
		return err
	}
	signatureBytes, err := base64.StdEncoding.DecodeString(signature.Signature)
	if err != nil || !ecdsa.VerifyASN1(publicKey, digest, signatureBytes) {
		return errors.New("detached report signature verification failed")
	}
	chainHash := sha256Hex([]byte(strings.Join(signature.CertificateChain, "\n")))
	if chainHash != signature.CertificateChainSHA256 {
		return errors.New("report certificate chain hash mismatch")
	}
	timestampToken, err := base64.StdEncoding.DecodeString(signature.TimestampToken)
	if err != nil || sha256Hex(timestampToken) != signature.TimestampTokenSHA256 {
		return errors.New("report timestamp token hash mismatch")
	}
	return nil
}

func (r *runner) archive(report reportResult, pdf []byte, technical technicalInput, market marketResult, valuation valuationResult) (string, error) {
	directory := filepath.Join(r.config.OutputDir, r.runID)
	if err := os.MkdirAll(directory, 0o700); err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(directory, "report.pdf"), pdf, 0o600); err != nil {
		return "", err
	}
	signatureBytes, err := json.MarshalIndent(report.Signature, "", "  ")
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(directory, "signature.json"), append(signatureBytes, '\n'), 0o600); err != nil {
		return "", err
	}
	root := strings.TrimSpace(report.Signature.CertificateChain[len(report.Signature.CertificateChain)-1]) + "\n"
	if err := os.WriteFile(filepath.Join(directory, "local-test-root-ca.pem"), []byte(root), 0o644); err != nil {
		return "", err
	}
	manifest := map[string]any{
		"schemaVersion": "local_e2e.result_manifest.v1", "production": false, "runId": r.runID,
		"assessmentId": report.AssessmentID, "reportId": report.ReportID,
		"technicalReportId": technical.ReportID, "technicalReportSha256": technical.ReportSHA256,
		"technicalSnapshotId": technical.SnapshotID, "technicalSnapshotSha256": technical.SnapshotSHA256,
		"benchmarkMetrics": technical.BenchmarkMetrics, "marketSnapshotId": market.SnapshotID,
		"marketSnapshotSha256": market.SnapshotSHA256, "valuationId": valuation.ValuationID,
		"valuationSha256": valuation.ValuationSHA256, "reportPdfSha256": report.ReportPDFSHA256,
		"signatureAlgorithm": report.Signature.Algorithm, "signingKeyVersion": report.Signature.KeyVersion,
		"startedAt": r.startedAt, "completedAt": time.Now().UTC(),
	}
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(directory, "manifest.json"), append(manifestBytes, '\n'), 0o600); err != nil {
		return "", err
	}
	return directory, nil
}

func (r *runner) callService(ctx context.Context, method, path string, body any, credential serviceCredential, tenant, requestID string, expected int, output any) error {
	return r.callServiceWithReviewer(ctx, method, path, body, credential, tenant, requestID, "", expected, output)
}

func (r *runner) callServiceWithReviewer(ctx context.Context, method, path string, body any, credential serviceCredential, tenant, requestID, reviewer string, expected int, output any) error {
	headers := map[string]string{
		"Authorization": "Bearer " + credential.token, "X-Service-Subject": credential.subject,
		"X-Tenant-Ref": tenant, "X-Request-ID": requestID, "X-Correlation-ID": r.id("correlation"),
	}
	if method == http.MethodPost && requestID != "" {
		headers["Idempotency-Key"] = requestID
	}
	if reviewer != "" {
		headers["X-Reviewer-Ref"] = reviewer
	}
	return r.callEnvelope(ctx, method, r.config.AssessmentURL+path, body, headers, expected, output)
}

func (r *runner) expectServiceError(ctx context.Context, method, path string, body any, credential serviceCredential, tenant, requestID string, expected int, code string) error {
	headers := map[string]string{
		"Authorization": "Bearer " + credential.token, "X-Service-Subject": credential.subject,
		"X-Tenant-Ref": tenant, "X-Request-ID": requestID, "X-Correlation-ID": r.id("correlation"),
	}
	if method == http.MethodPost {
		headers["Idempotency-Key"] = requestID
	}
	status, response, err := r.request(ctx, method, r.config.AssessmentURL+path, body, headers)
	if err != nil {
		return err
	}
	var envelope apiEnvelope
	if json.Unmarshal(response, &envelope) != nil || status != expected || envelope.Error.Code != code {
		return fmt.Errorf("expected HTTP %d %s, got %d: %s", expected, code, status, strings.TrimSpace(string(response)))
	}
	return nil
}

func (r *runner) callEnvelope(ctx context.Context, method, endpoint string, body any, headers map[string]string, expected int, output any) error {
	status, response, err := r.request(ctx, method, endpoint, body, headers)
	if err != nil {
		return err
	}
	var envelope apiEnvelope
	if err := json.Unmarshal(response, &envelope); err != nil {
		return fmt.Errorf("decode %s: %w", endpoint, err)
	}
	if status != expected || !envelope.Success {
		return fmt.Errorf("%s %s returned HTTP %d (%s): %s", method, endpoint, status, envelope.Error.Code, envelope.Error.Message)
	}
	if output != nil {
		if err := json.Unmarshal(envelope.Data, output); err != nil {
			return fmt.Errorf("decode %s data: %w", endpoint, err)
		}
	}
	return nil
}

func (r *runner) request(ctx context.Context, method, endpoint string, body any, headers map[string]string) (int, []byte, error) {
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return 0, nil, err
		}
		reader = bytes.NewReader(encoded)
	}
	request, err := http.NewRequestWithContext(ctx, method, endpoint, reader)
	if err != nil {
		return 0, nil, err
	}
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	for name, value := range headers {
		request.Header.Set(name, value)
	}
	response, err := r.client.Do(request)
	if err != nil {
		return 0, nil, err
	}
	defer response.Body.Close()
	encoded, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	return response.StatusCode, encoded, err
}

func (r *runner) putBytes(ctx context.Context, method, endpoint string, headers map[string]string, content []byte) error {
	request, err := http.NewRequestWithContext(ctx, method, endpoint, bytes.NewReader(content))
	if err != nil {
		return err
	}
	request.ContentLength = int64(len(content))
	for name, value := range headers {
		if strings.EqualFold(name, "Content-Length") {
			continue
		}
		request.Header.Set(name, value)
	}
	response, err := r.client.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		body, _ := io.ReadAll(io.LimitReader(response.Body, 4096))
		return fmt.Errorf("object upload returned HTTP %d: %s", response.StatusCode, strings.TrimSpace(string(body)))
	}
	return nil
}

func (r *runner) getBytes(ctx context.Context, endpoint string, headers map[string]string) ([]byte, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	for name, value := range headers {
		request.Header.Set(name, value)
	}
	response, err := r.client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("object download returned HTTP %d", response.StatusCode)
	}
	return io.ReadAll(io.LimitReader(response.Body, 64<<20))
}

func (r *runner) id(suffix string) string { return "e2e-" + r.runID + "-" + r.phase + "-" + suffix }

func randomID() (string, error) {
	value := make([]byte, 4)
	if _, err := rand.Read(value); err != nil {
		return "", err
	}
	return hex.EncodeToString(value), nil
}

func sha256Hex(value []byte) string {
	digest := sha256.Sum256(value)
	return hex.EncodeToString(digest[:])
}

func contains(values []string, candidate string) bool {
	for _, value := range values {
		if value == candidate {
			return true
		}
	}
	return false
}

func envDefault(name, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(name)); value != "" {
		return value
	}
	return fallback
}

func firstEnvironment(names ...string) string {
	for _, name := range names {
		if value := strings.TrimSpace(os.Getenv(name)); value != "" {
			return value
		}
	}
	return ""
}

func envBool(name string, fallback bool) (bool, error) {
	raw := strings.TrimSpace(os.Getenv(name))
	if raw == "" {
		return fallback, nil
	}
	value, err := strconv.ParseBool(raw)
	if err != nil {
		return false, fmt.Errorf("%s must be a boolean", name)
	}
	return value, nil
}

func validateServiceURL(raw string, allowContainerHTTP bool) error {
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Host == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return fmt.Errorf("invalid E2E service URL %q", raw)
	}
	if parsed.Scheme == "https" {
		return nil
	}
	if parsed.Scheme != "http" {
		return errors.New("E2E service URLs must use HTTP or HTTPS")
	}
	hostname := strings.ToLower(parsed.Hostname())
	if hostname == "localhost" {
		return nil
	}
	if ip := net.ParseIP(hostname); ip != nil {
		if ip.IsLoopback() || (allowContainerHTTP && ip.IsPrivate()) {
			return nil
		}
		return errors.New("plain HTTP E2E service URLs may not use a public IP address")
	}
	if allowContainerHTTP && !strings.Contains(hostname, ".") {
		return nil
	}
	return errors.New("plain HTTP E2E service URLs require loopback or E2E_ALLOW_CONTAINER_HTTP=true with a private container host")
}

func isLoopbackHTTP(raw string) bool {
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Scheme != "http" {
		return false
	}
	hostname := strings.ToLower(parsed.Hostname())
	if hostname == "localhost" {
		return true
	}
	ip := net.ParseIP(hostname)
	return ip != nil && ip.IsLoopback()
}
