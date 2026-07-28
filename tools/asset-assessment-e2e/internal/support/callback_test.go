package support

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestCallbackSinkVerifiesReplayConflictAndTampering(t *testing.T) {
	const secret = "local-callback-test-secret-32-bytes-minimum"
	now := time.Date(2026, 7, 28, 1, 0, 0, 0, time.UTC)
	sink := NewCallbackSink(secret)
	sink.now = func() time.Time { return now }
	event := callbackFixture("EVT-CALLBACK-1", "ASMT-CALLBACK-1", "evidence_pending")
	body, _ := json.Marshal(event)
	timestamp := strconv.FormatInt(now.Unix(), 10)
	signature := callbackSignature([]byte(secret), event.EventID, timestamp, body)

	response := callCallback(sink, event.EventID, timestamp, signature, body)
	if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"duplicate":false`) {
		t.Fatalf("valid callback failed: %d %s", response.Code, response.Body.String())
	}
	response = callCallback(sink, event.EventID, timestamp, signature, body)
	if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"duplicate":true`) {
		t.Fatalf("callback replay was not idempotent: %d %s", response.Code, response.Body.String())
	}

	event.Status = "reviewing"
	conflictingBody, _ := json.Marshal(event)
	response = callCallback(sink, event.EventID, timestamp, callbackSignature([]byte(secret), event.EventID, timestamp, conflictingBody), conflictingBody)
	if response.Code != http.StatusConflict || !strings.Contains(response.Body.String(), "CALLBACK_EVENT_CONFLICT") {
		t.Fatalf("callback conflict was not rejected: %d %s", response.Code, response.Body.String())
	}
	response = callCallback(sink, event.EventID, timestamp, signature, []byte(`{"tampered":true}`))
	if response.Code != http.StatusForbidden {
		t.Fatalf("tampered callback was not rejected: %d %s", response.Code, response.Body.String())
	}
	stats := sink.Stats()
	if stats.AcceptedCount != 1 || stats.ReplayCount != 1 || stats.ConflictCount != 1 || !containsStatus(stats.Assessments[event.AssessmentID], "evidence_pending") {
		t.Fatalf("unexpected callback stats: %+v", stats)
	}
}

func TestCallbackSinkRejectsExpiredAndDuplicateHeaders(t *testing.T) {
	const secret = "local-callback-test-secret-32-bytes-minimum"
	now := time.Date(2026, 7, 28, 1, 0, 0, 0, time.UTC)
	sink := NewCallbackSink(secret)
	sink.now = func() time.Time { return now }
	event := callbackFixture("EVT-CALLBACK-2", "ASMT-CALLBACK-2", "created")
	body, _ := json.Marshal(event)
	expired := strconv.FormatInt(now.Add(-301*time.Second).Unix(), 10)
	response := callCallback(sink, event.EventID, expired, callbackSignature([]byte(secret), event.EventID, expired, body), body)
	if response.Code != http.StatusUnauthorized {
		t.Fatalf("expired callback was not rejected: %d", response.Code)
	}
	timestamp := strconv.FormatInt(now.Unix(), 10)
	request := callbackRequest(event.EventID, timestamp, callbackSignature([]byte(secret), event.EventID, timestamp, body), body)
	request.Header.Add("X-Event-ID", event.EventID)
	response = httptest.NewRecorder()
	sink.Receive(response, request)
	if response.Code != http.StatusUnauthorized {
		t.Fatalf("duplicate callback header was not rejected: %d", response.Code)
	}
}

func callbackFixture(eventID, assessmentID, status string) callbackEvent {
	return callbackEvent{
		EventID: eventID, EventType: "asset_assessment.status_changed", SchemaVersion: "asset_assessment_event.v1",
		OccurredAt: time.Date(2026, 7, 28, 1, 0, 0, 0, time.UTC), CorrelationID: "corr-callback",
		AssessmentID: assessmentID, ClientRequestID: "client-callback", AssetRef: "asset-callback",
		Status: status, Progress: 30, RequiredEvidenceCodes: []string{},
		Report: json.RawMessage("null"), Error: json.RawMessage("null"),
	}
}

func callCallback(sink *CallbackSink, eventID, timestamp, signature string, body []byte) *httptest.ResponseRecorder {
	response := httptest.NewRecorder()
	sink.Receive(response, callbackRequest(eventID, timestamp, signature, body))
	return response
}

func callbackRequest(eventID, timestamp, signature string, body []byte) *http.Request {
	request := httptest.NewRequest(http.MethodPost, callbackPath, bytes.NewReader(body))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Event-ID", eventID)
	request.Header.Set("X-Event-Timestamp", timestamp)
	request.Header.Set("X-Event-Signature", signature)
	return request
}

func callbackSignature(secret []byte, eventID, timestamp string, body []byte) string {
	digest := sha256.Sum256(body)
	canonical := http.MethodPost + "\n" + callbackPath + "\n" + eventID + "\n" + timestamp + "\n" + hex.EncodeToString(digest[:])
	mac := hmac.New(sha256.New, secret)
	_, _ = mac.Write([]byte(canonical))
	return "v1=" + hex.EncodeToString(mac.Sum(nil))
}

func containsStatus(values []string, candidate string) bool {
	for _, value := range values {
		if value == candidate {
			return true
		}
	}
	return false
}
