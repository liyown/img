package config

import (
	"os"
	"path/filepath"
	"runtime"
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
	if c.Providers["x"].Headers["Authorization"] != "Bearer ${IMG_TEST_TOKEN}" {
		t.Fatal("config load must preserve credential references")
	}
	resolved, err := ResolveProvider(c.Providers["x"])
	if err != nil || resolved.Headers["Authorization"] != "Bearer top-secret" {
		t.Fatalf("selected provider credential was not expanded: %v", err)
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
	c.Providers["x"] = ProviderConfig{Type: "http", Token: "ghp_abcdef", SecretKey: "secret", Headers: map[string]string{"Authorization": "Bearer private", "X-Api-Key": "api-private"}}
	r := Redact(c)
	b := r.Providers["x"]
	if strings.Contains(b.Token, "abcdef") || strings.Contains(b.SecretKey, "secret") || strings.Contains(b.Headers["Authorization"], "private") || strings.Contains(b.Headers["X-Api-Key"], "private") {
		t.Fatal("secret was not redacted")
	}
}

func TestProjectConfigCannotDefineProviders(t *testing.T) {
	d := t.TempDir()
	g := filepath.Join(d, "global.toml")
	p := filepath.Join(d, "project.toml")
	os.WriteFile(g, []byte("version=1\n"), 0600)
	os.WriteFile(p, []byte("[providers.evil]\ntype='http'\nurl='http://127.0.0.1'\nurl_json_path='url'\n"), 0600)
	if _, err := Load(g, p); err == nil {
		t.Fatal("project provider injection was accepted")
	}
}

func TestSaveTightensPermissions(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("POSIX mode bits")
	}
	p := filepath.Join(t.TempDir(), "config.toml")
	if err := os.WriteFile(p, []byte("version=1\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := Save(p, Defaults()); err != nil {
		t.Fatal(err)
	}
	st, err := os.Stat(p)
	if err != nil {
		t.Fatal(err)
	}
	if st.Mode().Perm() != 0600 {
		t.Fatalf("mode=%o", st.Mode().Perm())
	}
}

func TestRejectsPlaintextCredentialsByDefault(t *testing.T) {
	c := Defaults()
	c.Providers["gh"] = ProviderConfig{Type: "github", Owner: "o", Repo: "r", Token: "plain-token"}
	if err := c.Validate(); err == nil {
		t.Fatal("plaintext credential accepted")
	}
	c.AllowPlaintextCredentials = true
	if err := c.Validate(); err != nil {
		t.Fatal(err)
	}
}

func TestUnselectedProviderMayHaveMissingCredential(t *testing.T) {
	d := t.TempDir()
	p := filepath.Join(d, "config.toml")
	os.WriteFile(p, []byte("version=1\n[providers.a]\ntype='github'\nowner='o'\nrepo='r'\ntoken='${MISSING_TOKEN}'\n"), 0600)
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
