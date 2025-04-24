package server

import (
	"context"
	"errors"
	"fmt"
	"io"
	"io/ioutil"
	"net"
	"os"
)

var (
	// ErrNetConnAccept indicates that TCP listener fails to accept connections
	ErrNetConnAccept = errors.New("failed to accept TCP connection")
	// ErrNetConnWrite indicates that a response could not be written
	ErrNetConnWrite = errors.New("failed to write a response")
)

// Server interface to be implemented to enable serving via TCP from a local file or fetching remotely
type Server interface {
	Serve(context.Context) (errChan <-chan error, err error)
}

// server actual implementation
// fileOrigin is a path to the local file or address in the form of address:port
// bindAddress where local TCP server is listening
// content caches file content to avoid refetching the file content on every connection
type server struct {
	fileOrigin  string
	bindAddress string

	listener *net.TCPListener
	content  []byte
}

// NewServer does validation on fileOrigin and bindAddress passed arguments
// fileOrigin should be a valid address of format host:port or path to the file
// bindAddress should be a valid address of format host:port
func NewServer(fileOrigin, bindAddress string) (Server, error) {
	if !isHostPort(bindAddress) {
		return nil, fmt.Errorf("Invalid bind address: %s", bindAddress)
	}

	s := &server{
		fileOrigin:  fileOrigin,
		bindAddress: bindAddress,
	}

	var f io.ReadCloser
	var err error

	if isHostPort(fileOrigin) {
		f, err = net.Dial("tcp", fileOrigin)
	} else {
		f, err = os.Open(s.fileOrigin)
	}

	if err != nil {
		return nil, err
	}
	defer f.Close()

	s.content, err = ioutil.ReadAll(f)
	if err != nil {
		return nil, err
	}

	return s, nil
}

// Serve starts TCP listener on the bindAddress
// Two error types can be retrieved from error channel:
// ErrNetConnAccept is pushed to the channel when TCP listener fails to accept connections
// ErrNetConnWrite is push to the channel when response could not be written
func (s *server) Serve(ctx context.Context) (<-chan error, error) {
	errChan := make(chan error)
	var err error

	addr, err := net.ResolveTCPAddr("tcp", s.bindAddress)
	if err != nil {
		return nil, err
	}

	s.listener, err = net.ListenTCP("tcp", addr)
	if err != nil {
		return nil, err
	}

	go func() {
		defer s.listener.Close()

		for {
			select {
			case <-ctx.Done():
				return
			default:
				conn, err := s.listener.Accept()
				if err != nil {
					errChan <- ErrNetConnAccept
					return
				}
				go func() {
					if _, err := conn.Write(s.content); err != nil {
						errChan <- ErrNetConnWrite
					}
					conn.Close()
				}()
			}
		}
	}()

	return errChan, nil
}

func isHostPort(hostport string) bool {
	_, err := net.ResolveTCPAddr("tcp", hostport)
	return err == nil
}
