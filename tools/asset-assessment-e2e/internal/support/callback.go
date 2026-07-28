package support

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

const callbackPath = "/api/banking/callback/assessment"

type CallbackSink struct {
	secret      []byte
	now         func() time.Time
	mu          sync.Mutex
	events      map[string]string
	assessments map[string]map[string]struct{}
	accepted    int
	replayed    int
	conflicted  int
}

type callbackEvent struct {
	EventID               string          `json:"eventId"`
	EventType             string          `json:"eventType"`
	SchemaVersion         string          `json:"schemaVersion"`
	OccurredAt            time.Time       `json:"occurredAt"`
	CorrelationID         string          `json:"correlationId"`
	AssessmentID          string          `json:"assessmentId"`
	ClientRequestID       string          `json:"clientRequestId"`
	AssetRef              string          `json:"assetRef"`
	Status                string          `json:"status"`
	Progress              int             `json:"progress"`
	RequiredEvidenceCodes []string        `json:"requiredEvidenceCodes"`
	Report                json.RawMessage `json:"report"`
	Error                 json.RawMessage `json:"error"`
}

type CallbackStats struct {
	AcceptedCount int                 `json:"acceptedCount"`
	ReplayCount   int                 `json:"replayCount"`
	ConflictCount int                 `json:"conflictCount"`
	Assessments   map[string][]string `json:"assessments"`
}

func NewCallbackSink(secret string) *CallbackSink {
	return &CallbackSink{
		secret: []byte(secret), now: func() time.Time { return time.Now().UTC() },
		events: make(map[string]string), assessments: make(map[string]map[string]struct{}),
	}
}

func (sink *CallbackSink) Receive(writer http.ResponseWriter, request *http.Request) {
	if sink == nil || len(sink.secret) < 32 || len(sink.secret) > 4096 {
		writeCallbackError(writer, http.StatusServiceUnavailable, "SECURITY_CONFIGURATION_MISSING")
		return
	}
	eventIDs := request.Header.Values("X-Event-ID")
	timestamps := request.Header.Values("X-Event-Timestamp")
	signatures := request.Header.Values("X-Event-Signature")
	if len(eventIDs) != 1 || len(timestamps) != 1 || len(signatures) != 1 {
		writeCallbackError(writer, http.StatusUnauthorized, "UNAUTHENTICATED")
		return
	}
	eventID, timestamp, signature := eventIDs[0], timestamps[0], signatures[0]
	if eventID == "" || eventID != strings.TrimSpace(eventID) || len(eventID) > 128 || strings.Contains(eventID, ",") ||
		timestamp == "" || timestamp != strings.TrimSpace(timestamp) ||
		signature != strings.TrimSpace(signature) || len(signature) != 67 || !strings.HasPrefix(signature, "v1=") {
		writeCallbackError(writer, http.StatusUnauthorized, "UNAUTHENTICATED")
		return
	}
	signatureHex := strings.TrimPrefix(signature, "v1=")
	signatureBytes, err := hex.DecodeString(signatureHex)
	if err != nil || signatureHex != strings.ToLower(signatureHex) || len(signatureBytes) != sha256.Size {
		writeCallbackError(writer, http.StatusUnauthorized, "UNAUTHENTICATED")
		return
	}
	timestampSeconds, err := strconv.ParseInt(timestamp, 10, 64)
	if err != nil || strconv.FormatInt(timestampSeconds, 10) != timestamp {
		writeCallbackError(writer, http.StatusUnauthorized, "UNAUTHENTICATED")
		return
	}
	now := sink.now().Unix()
	if timestampSeconds < now-300 || timestampSeconds > now+300 {
		writeCallbackError(writer, http.StatusUnauthorized, "UNAUTHENTICATED")
		return
	}
	body, err := io.ReadAll(io.LimitReader(request.Body, (1<<20)+1))
	if err != nil {
		writeCallbackError(writer, http.StatusBadRequest, "INVALID_REQUEST")
		return
	}
	if len(body) > 1<<20 {
		writeCallbackError(writer, http.StatusRequestEntityTooLarge, "PAYLOAD_TOO_LARGE")
		return
	}
	payloadHash := sha256.Sum256(body)
	canonical := http.MethodPost + "\n" + callbackPath + "\n" + eventID + "\n" + timestamp + "\n" + hex.EncodeToString(payloadHash[:])
	mac := hmac.New(sha256.New, sink.secret)
	_, _ = mac.Write([]byte(canonical))
	if !hmac.Equal(signatureBytes, mac.Sum(nil)) {
		writeCallbackError(writer, http.StatusForbidden, "FORBIDDEN")
		return
	}
	var event callbackEvent
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&event); err != nil {
		writeCallbackError(writer, http.StatusBadRequest, "INVALID_REQUEST")
		return
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) ||
		event.EventID != eventID || event.EventType != "asset_assessment.status_changed" ||
		event.SchemaVersion != "asset_assessment_event.v1" || event.AssessmentID == "" ||
		event.ClientRequestID == "" || event.AssetRef == "" || event.Status == "" ||
		event.Progress < 0 || event.Progress > 100 {
		writeCallbackError(writer, http.StatusBadRequest, "INVALID_REQUEST")
		return
	}
	digest := hex.EncodeToString(payloadHash[:])
	sink.mu.Lock()
	defer sink.mu.Unlock()
	if existing, found := sink.events[eventID]; found {
		if existing != digest {
			sink.conflicted++
			writeCallbackError(writer, http.StatusConflict, "CALLBACK_EVENT_CONFLICT")
			return
		}
		sink.replayed++
		writeCallbackSuccess(writer, true)
		return
	}
	sink.events[eventID] = digest
	if sink.assessments[event.AssessmentID] == nil {
		sink.assessments[event.AssessmentID] = make(map[string]struct{})
	}
	sink.assessments[event.AssessmentID][event.Status] = struct{}{}
	sink.accepted++
	writeCallbackSuccess(writer, false)
}

func (sink *CallbackSink) Stats() CallbackStats {
	sink.mu.Lock()
	defer sink.mu.Unlock()
	stats := CallbackStats{
		AcceptedCount: sink.accepted, ReplayCount: sink.replayed,
		ConflictCount: sink.conflicted, Assessments: make(map[string][]string, len(sink.assessments)),
	}
	for assessmentID, values := range sink.assessments {
		statuses := make([]string, 0, len(values))
		for status := range values {
			statuses = append(statuses, status)
		}
		sort.Strings(statuses)
		stats.Assessments[assessmentID] = statuses
	}
	return stats
}

func writeCallbackSuccess(writer http.ResponseWriter, duplicate bool) {
	writeJSON(writer, http.StatusOK, map[string]any{
		"success": true, "code": "EVENT_ACCEPTED", "message": "",
		"data": map[string]bool{"duplicate": duplicate}, "requestId": "",
	})
}

func writeCallbackError(writer http.ResponseWriter, status int, code string) {
	writeJSON(writer, status, map[string]any{
		"success": false, "code": code, "message": code, "retryable": status >= 500, "requestId": "",
	})
}
