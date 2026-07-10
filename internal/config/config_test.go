package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLoadPrecedenceAndExpansion(t *testing.T) {
	d := t.TempDir()
	g := filepath.Join(d, "global.toml")
	p := filepath.Join(d, "project.toml")
	os.WriteFile(g, []byte("version=1\ndefault_provider='x'\n[output]\nformat='url'\n[providers.x]\ntype='http'\nurl='https://example.test'\nurl_json_path='url'\n[providers.x.headers]\nAuthorization='Bearer ${IMG_TEST_TOKEN}'\n"), 0600)
	os.WriteFile(p, []byte("provider='x'\n[output]\nformat='markdown'\n"), 0600)
	t.Setenv("IMG_TEST_TOKEN", "top-secret")
	t.Setenv("IMG_OUTPUT_FORMAT", "json")
	c, err := Load(g, p)
	if err != nil {
		t.Fatal(err)
	}
	if c.Output.Format != "json" || c.Provider != "x" {
		t.Fatalf("precedence failed: %+v", c)
	}
	if c.Providers["x"].Headers["Authorization"] != "Bearer top-secret" {
		t.Fatal("placeholder not expanded")
	}
}
func TestValidationAndRedaction(t *testing.T) {
	c := Defaults()
	c.Version = 2
	if err := c.Validate(); err == nil {
		t.Fatal("expected version error")
	}
	c = Defaults()
	c.Providers["x"] = ProviderConfig{Type: "wat"}
	if err := c.Validate(); err == nil {
		t.Fatal("expected provider error")
	}
	c.Providers["x"] = ProviderConfig{Type: "http", Token: "ghp_abcdef", SecretKey: "secret", Headers: map[string]string{"Authorization": "Bearer private"}}
	r := Redact(c)
	b := r.Providers["x"]
	if strings.Contains(b.Token, "abcdef") || strings.Contains(b.SecretKey, "secret") || strings.Contains(b.Headers["Authorization"], "private") {
		t.Fatal("secret was not redacted")
	}
}

func TestUnselectedProviderMayHaveMissingCredential(t *testing.T) {
	d := t.TempDir()
	p := filepath.Join(d, "config.toml")
	os.WriteFile(p, []byte("version=1\n[providers.a]\ntype='github'\ntoken='${MISSING_TOKEN}'\n"), 0600)
	c, err := Load(p, "")
	if err != nil {
		t.Fatal(err)
	}
	if c.Providers["a"].Token != "${MISSING_TOKEN}" {
		t.Fatalf("reference changed: %q", c.Providers["a"].Token)
	}
	if _, err := ResolveProvider(c.Providers["a"]); err == nil {
		t.Fatal("selected provider must require its credential")
	}
}
