package support

import (
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"strings"
)

type HTTPServer struct {
	token    []byte
	maxHTML  int64
	renderer PDFRenderer
	signer   ReportSigner
	scanner  *EvidenceScanner
	malware  MalwareScanner
	callback *CallbackSink
	mux      *http.ServeMux
}

func NewHTTPServer(token string, maxHTML int64, renderer PDFRenderer, signer ReportSigner, scanner *EvidenceScanner, malware MalwareScanner, callback *CallbackSink) *HTTPServer {
	server := &HTTPServer{
		token: []byte(token), maxHTML: maxHTML, renderer: renderer,
		signer: signer, scanner: scanner, malware: malware, callback: callback, mux: http.NewServeMux(),
	}
	server.routes()
	return server
}

func (server *HTTPServer) Handler() http.Handler { return server.mux }

func (server *HTTPServer) routes() {
	server.mux.HandleFunc("GET /healthz", server.health)
	server.mux.Handle("GET /internal/v1/test-trust", server.authenticate(http.HandlerFunc(server.trust)))
	server.mux.Handle("POST /internal/v1/pdf-renders", server.authenticate(http.HandlerFunc(server.render)))
	server.mux.Handle("POST /internal/v1/report-signatures", server.authenticate(http.HandlerFunc(server.sign)))
	server.mux.Handle("POST /internal/v1/evidence-scans", server.authenticate(http.HandlerFunc(server.scan)))
	server.mux.HandleFunc("POST "+callbackPath, server.callback.Receive)
	server.mux.Handle("GET /internal/v1/callback-state", server.authenticate(http.HandlerFunc(server.callbackState)))
}

func (server *HTTPServer) callbackState(writer http.ResponseWriter, _ *http.Request) {
	writeJSON(writer, http.StatusOK, map[string]any{"success": true, "data": server.callback.Stats()})
}

func (server *HTTPServer) authenticate(next http.Handler) http.Handler {
	return http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		supplied := request.Header.Get("Authorization")
		if !strings.HasPrefix(supplied, "Bearer ") ||
			subtle.ConstantTimeCompare(server.token, []byte(strings.TrimPrefix(supplied, "Bearer "))) != 1 {
			writeError(writer, http.StatusUnauthorized, "UNAUTHENTICATED", "service authentication failed")
			return
		}
		next.ServeHTTP(writer, request)
	})
}

func (server *HTTPServer) health(writer http.ResponseWriter, request *http.Request) {
	if err := server.malware.Ping(request.Context()); err != nil {
		writeError(writer, http.StatusServiceUnavailable, "DEPENDENCY_UNAVAILABLE", "ClamAV is unavailable")
		return
	}
	writeJSON(writer, http.StatusOK, map[string]any{
		"success": true,
		"data": map[string]any{
			"status": "ready", "mode": "non-production-local-e2e",
			"keyVersion": server.signer.PublicInfo().KeyVersion,
		},
	})
}

func (server *HTTPServer) trust(writer http.ResponseWriter, _ *http.Request) {
	writeJSON(writer, http.StatusOK, map[string]any{"success": true, "data": server.signer.PublicInfo()})
}

type renderRequest struct {
	ReportID   string `json:"reportId"`
	HTML       string `json:"html"`
	HTMLSHA256 string `json:"htmlSha256"`
}

func (server *HTTPServer) render(writer http.ResponseWriter, request *http.Request) {
	var input renderRequest
	if !decodeJSON(writer, request, server.maxHTML+4096, &input) {
		return
	}
	if input.ReportID == "" || len(input.HTML) == 0 || int64(len(input.HTML)) > server.maxHTML ||
		len(input.HTMLSHA256) != 64 || !validateHTMLHash(input.HTML, strings.ToLower(input.HTMLSHA256)) {
		writeError(writer, http.StatusUnprocessableEntity, "RENDER_INPUT_INVALID", "render input integrity check failed")
		return
	}
	pdf, err := server.renderer.Render(request.Context(), input.ReportID, input.HTML)
	if err != nil {
		writeError(writer, http.StatusServiceUnavailable, "RENDER_FAILED", "PDF renderer failed")
		return
	}
	digest := sha256.Sum256(pdf)
	writeJSON(writer, http.StatusOK, map[string]any{
		"success": true,
		"data": map[string]any{
			"pdfBase64": base64.StdEncoding.EncodeToString(pdf),
			"pdfSha256": hex.EncodeToString(digest[:]),
		},
	})
}

type signingRequest struct {
	ReportID     string `json:"reportId"`
	DigestSHA256 string `json:"digestSha256"`
	DigestBase64 string `json:"digestBase64"`
}

func (server *HTTPServer) sign(writer http.ResponseWriter, request *http.Request) {
	var input signingRequest
	if !decodeJSON(writer, request, 64<<10, &input) {
		return
	}
	digest, err := base64.StdEncoding.DecodeString(input.DigestBase64)
	if err != nil || input.ReportID == "" || len(digest) != sha256.Size ||
		hex.EncodeToString(digest) != strings.ToLower(input.DigestSHA256) {
		writeError(writer, http.StatusUnprocessableEntity, "SIGNING_INPUT_INVALID", "signing digest integrity check failed")
		return
	}
	signature, err := server.signer.Sign(request.Context(), input.DigestSHA256)
	if err != nil {
		writeError(writer, http.StatusServiceUnavailable, "SIGNING_FAILED", "local HSM signing failed")
		return
	}
	writeJSON(writer, http.StatusOK, map[string]any{"success": true, "data": signature})
}

func (server *HTTPServer) scan(writer http.ResponseWriter, request *http.Request) {
	var input ScanRequest
	if !decodeJSON(writer, request, 128<<10, &input) {
		return
	}
	result, err := server.scanner.Scan(request.Context(), input)
	if err != nil {
		log.Printf("evidence scan %q failed: %v", input.JobID, err)
		writeError(writer, http.StatusUnprocessableEntity, "SCAN_FAILED", "evidence scan failed closed")
		return
	}
	writeJSON(writer, http.StatusOK, map[string]any{"success": true, "data": result})
}

func decodeJSON(writer http.ResponseWriter, request *http.Request, maximum int64, value any) bool {
	defer request.Body.Close()
	decoder := json.NewDecoder(io.LimitReader(request.Body, maximum+1))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		writeError(writer, http.StatusBadRequest, "INVALID_JSON", "request JSON is invalid")
		return false
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		writeError(writer, http.StatusBadRequest, "INVALID_JSON", "request must contain one JSON value")
		return false
	}
	return true
}

func writeJSON(writer http.ResponseWriter, status int, value any) {
	writer.Header().Set("Content-Type", "application/json")
	writer.Header().Set("Cache-Control", "no-store")
	writer.WriteHeader(status)
	_ = json.NewEncoder(writer).Encode(value)
}

func writeError(writer http.ResponseWriter, status int, code, message string) {
	writeJSON(writer, status, map[string]any{
		"success": false,
		"error":   map[string]any{"code": code, "message": message, "retryable": status >= 500},
	})
}
