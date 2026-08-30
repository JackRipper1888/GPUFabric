package support

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestValidateCJKFont(t *testing.T) {
	directory := t.TempDir()
	fontMatch := filepath.Join(directory, "fc-match")
	if err := os.WriteFile(fontMatch, []byte("#!/bin/sh\nprintf '%s\\n' 'NotoSansCJK-Regular.ttc: Noto Sans CJK SC'\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := ValidateCJKFont(context.Background(), fontMatch); err != nil {
		t.Fatalf("expected CJK font validation to pass: %v", err)
	}
}

func TestValidateCJKFontRejectsFallbackFont(t *testing.T) {
	directory := t.TempDir()
	fontMatch := filepath.Join(directory, "fc-match")
	if err := os.WriteFile(fontMatch, []byte("#!/bin/sh\nprintf '%s\\n' 'LiberationSans-Regular.ttf: Liberation Sans'\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := ValidateCJKFont(context.Background(), fontMatch); err == nil {
		t.Fatal("expected fallback-only runtime to be rejected")
	}
}
