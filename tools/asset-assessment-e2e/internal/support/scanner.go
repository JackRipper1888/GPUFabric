package support

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type ScanRequest struct {
	JobID                 string            `json:"jobId"`
	DownloadMethod        string            `json:"downloadMethod"`
	DownloadURL           string            `json:"downloadUrl"`
	RequiredHeaders       map[string]string `json:"requiredHeaders"`
	ExpectedSHA256        string            `json:"expectedSha256"`
	ExpectedContentType   string            `json:"expectedContentType"`
	ExpectedContentLength int64             `json:"expectedContentLength"`
	FileName              string            `json:"fileName"`
}

type ScanResult struct {
	Status              string `json:"status"`
	DetectedContentType string `json:"detectedContentType"`
	SHA256              string `json:"sha256"`
	ReasonCode          string `json:"reasonCode,omitempty"`
	MalwareSignature    string `json:"malwareSignature,omitempty"`
	OCRText             string `json:"ocrText,omitempty"`
	OCRSHA256           string `json:"ocrSha256,omitempty"`
}

type OCRExtractor interface {
	Extract(ctx context.Context, content []byte, contentType string) (string, error)
}

type CLIExtractor struct {
	pdfToPPM  string
	tesseract string
	timeout   time.Duration
	runner    commandRunner
}

func NewCLIExtractor(pdfToPPM, tesseract string, timeout time.Duration) *CLIExtractor {
	return &CLIExtractor{pdfToPPM: pdfToPPM, tesseract: tesseract, timeout: timeout, runner: execCommandRunner{}}
}

func (extractor *CLIExtractor) Extract(ctx context.Context, content []byte, contentType string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, extractor.timeout)
	defer cancel()
	directory, err := os.MkdirTemp("", "assessment-ocr-")
	if err != nil {
		return "", err
	}
	defer os.RemoveAll(directory)
	inputPath := filepath.Join(directory, "input")
	if err := os.WriteFile(inputPath, content, 0o600); err != nil {
		return "", err
	}
	ocrPath := inputPath
	if canonicalContentType(contentType) == "application/pdf" {
		prefix := filepath.Join(directory, "page")
		if _, err := extractor.runner.Run(ctx, nil, nil, extractor.pdfToPPM,
			"-f", "1", "-singlefile", "-r", "150", "-png", inputPath, prefix); err != nil {
			return "", err
		}
		ocrPath = prefix + ".png"
	}
	output, err := extractor.runner.Run(ctx, nil, nil, extractor.tesseract,
		ocrPath, "stdout", "-l", "eng", "--psm", "6")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(output)), nil
}

type EvidenceScanner struct {
	client       *http.Client
	malware      MalwareScanner
	ocr          OCRExtractor
	allowedHosts map[string]struct{}
	maxBytes     int64
}

func NewEvidenceScanner(malware MalwareScanner, ocr OCRExtractor, allowedHosts map[string]struct{}, maxBytes int64, timeout time.Duration) *EvidenceScanner {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	client := &http.Client{
		Timeout:   timeout,
		Transport: transport,
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return errors.New("evidence download redirects are disabled")
		},
	}
	return &EvidenceScanner{client: client, malware: malware, ocr: ocr, allowedHosts: allowedHosts, maxBytes: maxBytes}
}

func (scanner *EvidenceScanner) Scan(ctx context.Context, request ScanRequest) (ScanResult, error) {
	if request.JobID == "" || request.DownloadMethod != http.MethodGet ||
		request.ExpectedContentLength <= 0 || request.ExpectedContentLength > scanner.maxBytes ||
		len(request.ExpectedSHA256) != 64 || request.ExpectedContentType == "" {
		return ScanResult{}, errors.New("invalid evidence scan request")
	}
	parsed, err := url.Parse(request.DownloadURL)
	if err != nil || parsed.User != nil || parsed.Fragment != "" ||
		(parsed.Scheme != "http" && parsed.Scheme != "https") {
		return ScanResult{}, errors.New("evidence download URL is not allowed")
	}
	if _, allowed := scanner.allowedHosts[strings.ToLower(parsed.Host)]; !allowed {
		return ScanResult{}, errors.New("evidence download host is not allowed")
	}
	download, err := http.NewRequestWithContext(ctx, http.MethodGet, parsed.String(), nil)
	if err != nil {
		return ScanResult{}, err
	}
	for name, value := range request.RequiredHeaders {
		if strings.EqualFold(strings.TrimSpace(name), "Host") {
			if !strings.EqualFold(strings.TrimSpace(value), parsed.Host) {
				return ScanResult{}, errors.New("evidence download Host header does not match the allowed URL")
			}
			continue
		}
		if !allowedEvidenceHeader(name) {
			return ScanResult{}, fmt.Errorf("evidence download header %q is not allowed", name)
		}
		download.Header.Set(name, value)
	}
	response, err := scanner.client.Do(download)
	if err != nil {
		return ScanResult{}, err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return ScanResult{}, fmt.Errorf("evidence download returned HTTP %d", response.StatusCode)
	}
	content, err := io.ReadAll(io.LimitReader(response.Body, scanner.maxBytes+1))
	if err != nil {
		return ScanResult{}, err
	}
	if int64(len(content)) != request.ExpectedContentLength {
		return ScanResult{}, errors.New("evidence content length does not match the trusted grant")
	}
	digest := sha256.Sum256(content)
	actualHash := hex.EncodeToString(digest[:])
	if !strings.EqualFold(actualHash, request.ExpectedSHA256) {
		return ScanResult{}, errors.New("evidence SHA-256 does not match the trusted grant")
	}
	detected := canonicalContentType(http.DetectContentType(content))
	malware, err := scanner.malware.Scan(ctx, content)
	if err != nil {
		return ScanResult{}, err
	}
	if malware.Infected {
		return ScanResult{
			Status: "infected", DetectedContentType: detected, SHA256: actualHash,
			ReasonCode: "MALWARE_DETECTED", MalwareSignature: malware.Signature,
		}, nil
	}
	result := ScanResult{Status: "clean", DetectedContentType: detected, SHA256: actualHash}
	if OCRSupported(detected) {
		text, err := scanner.ocr.Extract(ctx, content, detected)
		if err != nil {
			return ScanResult{}, err
		}
		ocrDigest := sha256.Sum256([]byte(text))
		result.OCRText = text
		result.OCRSHA256 = hex.EncodeToString(ocrDigest[:])
	}
	return result, nil
}

func allowedEvidenceHeader(name string) bool {
	switch strings.ToLower(strings.TrimSpace(name)) {
	case "x-amz-server-side-encryption-customer-algorithm",
		"x-amz-server-side-encryption-customer-key",
		"x-amz-server-side-encryption-customer-key-md5":
		return true
	default:
		return false
	}
}

func canonicalContentType(value string) string {
	value = strings.ToLower(strings.TrimSpace(value))
	if separator := strings.IndexByte(value, ';'); separator >= 0 {
		value = strings.TrimSpace(value[:separator])
	}
	return value
}

func OCRSupported(contentType string) bool {
	switch canonicalContentType(contentType) {
	case "application/pdf", "image/png", "image/jpeg", "image/tiff", "image/bmp":
		return true
	default:
		return false
	}
}
