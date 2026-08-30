package support

import (
	"context"
	"errors"
	"os/exec"
	"strings"
)

func ValidateCJKFont(ctx context.Context, fontMatchPath string) error {
	output, err := exec.CommandContext(ctx, fontMatchPath, "Noto Sans CJK SC").Output()
	if err != nil {
		return errors.New("Noto Sans CJK SC runtime check failed")
	}
	if !strings.Contains(string(output), "NotoSansCJK") {
		return errors.New("Noto Sans CJK SC is unavailable")
	}
	return nil
}
