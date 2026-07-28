package support

import (
	"bufio"
	"context"
	"encoding/binary"
	"io"
	"net"
	"testing"
	"time"
)

func TestClamDClientPingAndScan(t *testing.T) {
	address, stop := startFakeClamD(t, []string{"PONG", "stream: OK", "stream: Eicar-Signature FOUND"})
	defer stop()
	client := NewClamDClient(address, time.Second)
	if err := client.Ping(context.Background()); err != nil {
		t.Fatal(err)
	}
	clean, err := client.Scan(context.Background(), []byte("clean"))
	if err != nil || clean.Infected {
		t.Fatalf("unexpected clean result: %+v %v", clean, err)
	}
	infected, err := client.Scan(context.Background(), []byte("infected"))
	if err != nil || !infected.Infected || infected.Signature != "Eicar-Signature" {
		t.Fatalf("unexpected infected result: %+v %v", infected, err)
	}
}

func startFakeClamD(t *testing.T, responses []string) (string, func()) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		for _, response := range responses {
			connection, err := listener.Accept()
			if err != nil {
				return
			}
			reader := bufio.NewReader(connection)
			command, _ := reader.ReadString(0)
			if command == "zINSTREAM\x00" {
				for {
					var size [4]byte
					if _, err := io.ReadFull(reader, size[:]); err != nil {
						break
					}
					length := binary.BigEndian.Uint32(size[:])
					if length == 0 {
						break
					}
					_, _ = io.CopyN(io.Discard, reader, int64(length))
				}
			}
			_, _ = connection.Write(append([]byte(response), 0))
			_ = connection.Close()
		}
	}()
	return listener.Addr().String(), func() {
		_ = listener.Close()
		<-done
	}
}
