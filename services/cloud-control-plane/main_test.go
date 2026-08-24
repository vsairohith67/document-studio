package main

import (
	"bufio"
	"context"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestServerPolicy(t *testing.T) {
	server := newServer()
	if server.Addr != "127.0.0.1:8080" {
		t.Fatalf("unexpected bind address: %s", server.Addr)
	}
	if server.ReadHeaderTimeout != 5*time.Second {
		t.Fatalf("unexpected read header timeout: %s", server.ReadHeaderTimeout)
	}
	if server.ReadTimeout != 10*time.Second {
		t.Fatalf("unexpected read timeout: %s", server.ReadTimeout)
	}
	if server.WriteTimeout != 10*time.Second {
		t.Fatalf("unexpected write timeout: %s", server.WriteTimeout)
	}
	if server.IdleTimeout != 60*time.Second {
		t.Fatalf("unexpected idle timeout: %s", server.IdleTimeout)
	}
	if server.MaxHeaderBytes != 16<<10 {
		t.Fatalf("unexpected maximum header bytes: %d", server.MaxHeaderBytes)
	}
	if shutdownTimeout != 10*time.Second {
		t.Fatalf("unexpected shutdown timeout: %s", shutdownTimeout)
	}
}

func TestHealth(t *testing.T) {
	req := httptest.NewRequest("GET", "/healthz", nil)
	rec := httptest.NewRecorder()
	newMux().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	if !strings.Contains(rec.Body.String(), `"status":"ok"`) {
		t.Fatalf("unexpected body: %s", rec.Body.String())
	}
}

func TestHealthRejectsUnsupportedMethod(t *testing.T) {
	req := httptest.NewRequest("POST", "/healthz", nil)
	rec := httptest.NewRecorder()
	newMux().ServeHTTP(rec, req)
	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", rec.Code)
	}
}

func TestServerRejectsOversizedHeader(t *testing.T) {
	server := newServer()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	go func() {
		result <- runServer(ctx, server, listener)
	}()

	connection, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		cancel()
		<-result
		t.Fatal(err)
	}
	_, err = fmt.Fprintf(
		connection,
		"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Oversized: %s\r\n\r\n",
		strings.Repeat("a", maxHeaderBytes+(8<<10)),
	)
	if err != nil {
		_ = connection.Close()
		cancel()
		<-result
		t.Fatal(err)
	}
	_ = connection.SetReadDeadline(time.Now().Add(time.Second))
	response, err := http.ReadResponse(bufio.NewReader(connection), nil)
	_ = connection.Close()
	cancel()
	if shutdownErr := <-result; shutdownErr != nil {
		t.Fatalf("shutdown failed: %v", shutdownErr)
	}
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusRequestHeaderFieldsTooLarge {
		t.Fatalf("expected 431, got %d", response.StatusCode)
	}
}

func TestRunServerShutsDown(t *testing.T) {
	server := newServer()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	go func() {
		result <- runServer(ctx, server, listener)
	}()

	cancel()
	select {
	case err := <-result:
		if err != nil {
			t.Fatalf("unexpected shutdown error: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("server did not shut down")
	}
}
