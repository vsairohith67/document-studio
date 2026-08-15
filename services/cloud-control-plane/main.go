package main

import (
    "encoding/json"
    "log"
    "net/http"
)

type Health struct {
    Status string `json:"status"`
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

func main() {
    log.Println("listening on :8080")
    log.Fatal(http.ListenAndServe(":8080", newMux()))
}
