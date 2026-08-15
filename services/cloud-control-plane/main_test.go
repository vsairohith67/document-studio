package main

import (
    "net/http/httptest"
    "strings"
    "testing"
)

func TestHealth(t *testing.T) {
    req := httptest.NewRequest("GET", "/healthz", nil)
    rec := httptest.NewRecorder()
    newMux().ServeHTTP(rec, req)
    if rec.Code != 200 { t.Fatalf("expected 200, got %d", rec.Code) }
    if !strings.Contains(rec.Body.String(), `"status":"ok"`) { t.Fatalf("unexpected body: %s", rec.Body.String()) }
}
