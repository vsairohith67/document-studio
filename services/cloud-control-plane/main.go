package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

const (
	defaultListenAddress = "127.0.0.1:8080"
	readHeaderTimeout    = 5 * time.Second
	readTimeout          = 10 * time.Second
	writeTimeout         = 10 * time.Second
	idleTimeout          = 60 * time.Second
	shutdownTimeout      = 10 * time.Second
	maxHeaderBytes       = 16 << 10
)

type Health struct {
	Status  string `json:"status"`
	Service string `json:"service"`
}

func health(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(Health{Status: "ok", Service: "document-studio-cloud-control-plane"})
}

func newMux() *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", health)
	return mux
}

func newServer() *http.Server {
	return &http.Server{
		Addr:              defaultListenAddress,
		Handler:           newMux(),
		ReadHeaderTimeout: readHeaderTimeout,
		ReadTimeout:       readTimeout,
		WriteTimeout:      writeTimeout,
		IdleTimeout:       idleTimeout,
		MaxHeaderBytes:    maxHeaderBytes,
	}
}

func runServer(ctx context.Context, server *http.Server, listener net.Listener) error {
	serveResult := make(chan error, 1)
	go func() {
		serveResult <- server.Serve(listener)
	}()

	select {
	case err := <-serveResult:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	case <-ctx.Done():
		shutdownCtx, cancel := context.WithTimeout(context.Background(), shutdownTimeout)
		defer cancel()

		if err := server.Shutdown(shutdownCtx); err != nil {
			_ = server.Close()
			<-serveResult
			return fmt.Errorf("graceful shutdown: %w", err)
		}

		if err := <-serveResult; !errors.Is(err, http.ErrServerClosed) {
			return err
		}
		return nil
	}
}

func main() {
	server := newServer()
	listener, err := net.Listen("tcp", server.Addr)
	if err != nil {
		log.Fatal(err)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	log.Printf("listening on %s", server.Addr)
	if err := runServer(ctx, server, listener); err != nil {
		log.Fatal(err)
	}
}
