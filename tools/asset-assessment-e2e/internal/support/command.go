package support

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
)

type commandRunner interface {
	Run(ctx context.Context, environment []string, stdin []byte, name string, arguments ...string) ([]byte, error)
}

type execCommandRunner struct{}

func (execCommandRunner) Run(ctx context.Context, environment []string, stdin []byte, name string, arguments ...string) ([]byte, error) {
	command := exec.CommandContext(ctx, name, arguments...)
	command.Env = append(os.Environ(), environment...)
	if stdin != nil {
		command.Stdin = bytes.NewReader(stdin)
	}
	output, err := command.CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("%s failed: %w: %s", name, err, boundedCommandOutput(output))
	}
	return output, nil
}

func boundedCommandOutput(output []byte) string {
	const limit = 2048
	if len(output) > limit {
		output = output[len(output)-limit:]
	}
	return string(output)
}
