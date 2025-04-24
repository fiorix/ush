package server

import (
	"context"
	"fmt"
	"io/ioutil"
	"log"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"
)

var _ Server = &server{}

func TestNewServer(t *testing.T) {
	content := []byte("Discard after reading")
	l, openPort := createTestTCPListener(content)
	defer l.Close()

	f := createTempFile(content)
	defer os.Remove(f)

	for _, c := range []struct {
		fileOrigin  string
		bindAddress string
		expectError bool
	}{
		{
			fmt.Sprintf("localhost:%d", openPort),
			"example.com:8080",
			false,
		},
		{
			f,
			"example.com:8080",
			false,
		},
		{
			fmt.Sprintf("localhost:%d", openPort),
			"random",
			true,
		},
		{
			"localhost:200000",
			"example.com:8080",
			true,
		},
		{
			"random-file",
			"example.com:8080",
			true,
		},
	} {
		s, err := NewServer(c.fileOrigin, c.bindAddress)
		if c.expectError && err == nil {
			t.Fatalf("Expected error for %v: %v", c, err)
		}
		if !c.expectError && err != nil {
			t.Fatalf("Unexpected error for %v: %v", c, err)
		}
		if c.expectError {
			continue
		}
		scast := s.(*server)
		if string(scast.content) != string(content) {
			t.Errorf("Incorrectly cached content. Expected: %s, got: %s", string(content), string(scast.content))
		}
	}
}

func TestServe(t *testing.T) {
	t.Run("Test for file via proxy", testServeForFile)
	t.Run("Test network failure", testServeForFailure)
}

func testServeForFile(t *testing.T) {
	l, occupiedPort := createTestTCPListener([]byte(""))
	defer l.Close()

	fileContent := "love bash jkjk"
	f := createTempFile([]byte(fileContent))
	defer os.Remove(f)

	for _, c := range []struct {
		bindAddress string
		expectError bool
	}{
		{
			"localhost:0",
			false,
		},
		{
			fmt.Sprintf("localhost:%d", occupiedPort),
			true,
		},
	} {
		s, err := NewServer(f, c.bindAddress)
		if err != nil {
			t.Fatalf("Unexpected error when creating server object: %v", err)
		}

		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()

		_, err = s.Serve(ctx)
		if c.expectError && err == nil {
			t.Fatalf("Expected error for input %v", c)
		}
		if !c.expectError && err != nil {
			t.Fatalf("Unexpected error for input %v: %v", c, err)
		}
		if c.expectError {
			continue
		}

		scast := s.(*server)
		port := scast.listener.Addr().(*net.TCPAddr).Port
		conn, err := net.Dial("tcp", fmt.Sprintf("localhost:%d", port))
		if err != nil {
			t.Fatalf("Unexpected error trying to connect to TCP server: %v", err)
		}
		defer conn.Close()

		resp, _ := ioutil.ReadAll(conn)
		if string(resp) != fileContent {
			t.Errorf("Unexpected response: %s, expected: %s", string(resp), fileContent)
		}
	}
}

func testServeForFailure(t *testing.T) {
	l, occupiedPort := createTestTCPListener([]byte(""))
	defer l.Close()

	s, err := NewServer(fmt.Sprintf("localhost:%d", occupiedPort), "localhost:0")
	if err != nil {
		t.Fatalf("Unexpected error: %v", err)
	}

	scast := s.(*server) // manipulate the underlying object to close the TCP listener
	errChan, err := s.Serve(context.Background())

	if err != nil {
		t.Fatalf("Unexpected error: %v", err)
	}

	scast.listener.Close() // to fake network failure

	select {
	case err = <-errChan:
	case <-time.After(5 * time.Second): // TODO: change this hack
	}

	if err != ErrNetConnAccept {
		t.Errorf("Unexpected error was returned: %v", err)
	}
}

func TestIsHostPort(t *testing.T) {
	for _, c := range []struct {
		input string
		okay  bool
	}{
		{
			"[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:80",
			true,
		},
		{
			"0.0.0.0:0",
			true,
		},
		{
			"localhost:5050",
			true,
		},
		{
			"example.com:8080",
			true,
		},
		{
			"example.com:5050/path",
			false,
		},
		{
			"localhost",
			false,
		},
		{
			"name/file/path",
			false,
		},
		{
			"http://example.com:80",
			false,
		},
	} {
		if res := isHostPort(c.input); c.okay != res {
			t.Errorf("Unexpected result for input: %s, got: %v, expected: %v", c.input, res, c.okay)
		}
	}
}

/**
Helper functions
*/
func createTestTCPListener(resp []byte) (*net.TCPListener, int) {
	addr, _ := net.ResolveTCPAddr("tcp", "localhost:0")
	listener, _ := net.ListenTCP("tcp", addr)

	go func() {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		conn.Write(resp)
		conn.Close()
	}()

	return listener, listener.Addr().(*net.TCPAddr).Port
}

func createTempFile(content []byte) string {
	dir, _ := ioutil.TempDir("", "ush-test")
	tmpFile := filepath.Join(dir, "ush-test-file")

	if err := ioutil.WriteFile(tmpFile, content, 0666); err != nil {
		log.Fatal(err)
	}
	return tmpFile
}
