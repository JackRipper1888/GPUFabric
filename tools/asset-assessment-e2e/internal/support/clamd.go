package support

import (
	"bufio"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"strings"
	"time"
)

type MalwareResult struct {
	Infected  bool
	Signature string
}

type MalwareScanner interface {
	Ping(ctx context.Context) error
	Scan(ctx context.Context, content []byte) (MalwareResult, error)
}

type ClamDClient struct {
	address string
	timeout time.Duration
}

func NewClamDClient(address string, timeout time.Duration) *ClamDClient {
	return &ClamDClient{address: address, timeout: timeout}
}

func (client *ClamDClient) Ping(ctx context.Context) error {
	connection, err := client.dial(ctx)
	if err != nil {
		return err
	}
	defer connection.Close()
	if _, err := connection.Write([]byte("zPING\x00")); err != nil {
		return err
	}
	response, err := readClamDResponse(connection)
	if err != nil {
		return err
	}
	if response != "PONG" {
		return fmt.Errorf("clamd returned %q", response)
	}
	return nil
}

func (client *ClamDClient) Scan(ctx context.Context, content []byte) (MalwareResult, error) {
	connection, err := client.dial(ctx)
	if err != nil {
		return MalwareResult{}, err
	}
	defer connection.Close()
	if _, err := connection.Write([]byte("zINSTREAM\x00")); err != nil {
		return MalwareResult{}, err
	}
	for offset := 0; offset < len(content); {
		end := offset + 32*1024
		if end > len(content) {
			end = len(content)
		}
		var size [4]byte
		binary.BigEndian.PutUint32(size[:], uint32(end-offset))
		if _, err := connection.Write(size[:]); err != nil {
			return MalwareResult{}, err
		}
		if _, err := connection.Write(content[offset:end]); err != nil {
			return MalwareResult{}, err
		}
		offset = end
	}
	if _, err := connection.Write([]byte{0, 0, 0, 0}); err != nil {
		return MalwareResult{}, err
	}
	response, err := readClamDResponse(connection)
	if err != nil {
		return MalwareResult{}, err
	}
	if strings.HasSuffix(response, " OK") {
		return MalwareResult{}, nil
	}
	if strings.HasSuffix(response, " FOUND") {
		signature := strings.TrimSpace(strings.TrimSuffix(response, " FOUND"))
		if separator := strings.Index(signature, ":"); separator >= 0 {
			signature = strings.TrimSpace(signature[separator+1:])
		}
		return MalwareResult{Infected: true, Signature: signature}, nil
	}
	return MalwareResult{}, fmt.Errorf("clamd scan failed: %s", response)
}

func (client *ClamDClient) dial(ctx context.Context) (net.Conn, error) {
	dialer := &net.Dialer{Timeout: client.timeout}
	connection, err := dialer.DialContext(ctx, "tcp", client.address)
	if err != nil {
		return nil, err
	}
	if deadline, ok := ctx.Deadline(); ok {
		_ = connection.SetDeadline(deadline)
	} else {
		_ = connection.SetDeadline(time.Now().Add(client.timeout))
	}
	return connection, nil
}

func readClamDResponse(reader io.Reader) (string, error) {
	response, err := bufio.NewReader(io.LimitReader(reader, 4096)).ReadString(0)
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	response = strings.TrimSuffix(response, "\x00")
	response = strings.TrimSpace(response)
	if response == "" {
		return "", errors.New("clamd returned an empty response")
	}
	return response, nil
}
