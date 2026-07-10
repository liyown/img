package config

import (
	"bytes"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"

	"github.com/liyown/img/internal/credential"
	"github.com/pelletier/go-toml/v2"
)

type Config struct {
	Version                   int                       `toml:"version" json:"version"`
	DefaultProvider           string                    `toml:"default_provider" json:"default_provider"`
	Provider                  string                    `toml:"provider,omitempty" json:"provider,omitempty"`
	Output                    Output                    `toml:"output" json:"output"`
	Upload                    Upload                    `toml:"upload" json:"upload"`
	Providers                 map[string]ProviderConfig `toml:"providers" json:"providers"`
	AllowPlaintextCredentials bool                      `toml:"allow_plaintext_credentials,omitempty" json:"allow_plaintext_credentials,omitempty"`
}

type Output struct {
	Format string `toml:"format" json:"format"`
	Copy   bool   `toml:"copy" json:"copy"`
	Quiet  bool   `toml:"quiet" json:"quiet"`
}

type Upload struct {
	Path         string `toml:"path" json:"path"`
	PathTemplate string `toml:"path_template" json:"path_template"`
	Rename       string `toml:"rename" json:"rename"`
	Conflict     string `toml:"conflict" json:"conflict"`
	Overwrite    bool   `toml:"overwrite" json:"overwrite"`
	Concurrency  int    `toml:"concurrency" json:"concurrency"`
	MaxSize      int64  `toml:"max_size" json:"max_size"`
}

type ProviderConfig struct {
	Type          string            `toml:"type" json:"type"`
	Endpoint      string            `toml:"endpoint,omitempty" json:"endpoint,omitempty"`
	Region        string            `toml:"region,omitempty" json:"region,omitempty"`
	Bucket        string            `toml:"bucket,omitempty" json:"bucket,omitempty"`
	AccessKey     string            `toml:"access_key,omitempty" json:"access_key,omitempty"`
	SecretKey     string            `toml:"secret_key,omitempty" json:"secret_key,omitempty"`
	SessionToken  string            `toml:"session_token,omitempty" json:"session_token,omitempty"`
	PublicURL     string            `toml:"public_url,omitempty" json:"public_url,omitempty"`
	PathStyle     bool              `toml:"path_style,omitempty" json:"path_style,omitempty"`
	Owner         string            `toml:"owner,omitempty" json:"owner,omitempty"`
	Repo          string            `toml:"repo,omitempty" json:"repo,omitempty"`
	Branch        string            `toml:"branch,omitempty" json:"branch,omitempty"`
	Token         string            `toml:"token,omitempty" json:"token,omitempty"`
	CommitMessage string            `toml:"commit_message,omitempty" json:"commit_message,omitempty"`
	URL           string            `toml:"url,omitempty" json:"url,omitempty"`
	Method        string            `toml:"method,omitempty" json:"method,omitempty"`
	FileField     string            `toml:"file_field,omitempty" json:"file_field,omitempty"`
	URLJSONPath   string            `toml:"url_json_path,omitempty" json:"url_json_path,omitempty"`
	Headers       map[string]string `toml:"headers,omitempty" json:"headers,omitempty"`
	Fields        map[string]string `toml:"fields,omitempty" json:"fields,omitempty"`
	AllowInsecure bool              `toml:"allow_insecure,omitempty" json:"allow_insecure,omitempty"`
}

type projectConfig struct {
	Version  *int           `toml:"version"`
	Provider *string        `toml:"provider"`
	Output   *projectOutput `toml:"output"`
	Upload   *projectUpload `toml:"upload"`
}
type projectOutput struct {
	Format *string `toml:"format"`
}
type projectUpload struct {
	Path         *string `toml:"path"`
	PathTemplate *string `toml:"path_template"`
}

func Defaults() Config {
	return Config{Version: 1, Output: Output{Format: "url"}, Upload: Upload{PathTemplate: "{year}/{month}/{filename}", Rename: "original", Conflict: "error", Concurrency: 4, MaxSize: 20 << 20}, Providers: map[string]ProviderConfig{}}
}

func GlobalPath() (string, error) {
	d, err := os.UserConfigDir()
	if err != nil {
		return "", fmt.Errorf("locate user config directory: %w", err)
	}
	return filepath.Join(d, "img", "config.toml"), nil
}

func Load(globalPath, projectPath string) (Config, error) {
	c := Defaults()
	if globalPath != "" {
		b, err := os.ReadFile(globalPath)
		if errors.Is(err, os.ErrNotExist) {
			// A missing global config is valid.
		} else if err != nil {
			return c, fmt.Errorf("read config %s: %w", globalPath, err)
		} else if err := toml.Unmarshal(b, &c); err != nil {
			return c, fmt.Errorf("parse config %s: %w", globalPath, err)
		}
	}
	if projectPath != "" {
		b, err := os.ReadFile(projectPath)
		if errors.Is(err, os.ErrNotExist) {
			// A missing project config is valid.
		} else if err != nil {
			return c, fmt.Errorf("read config %s: %w", projectPath, err)
		} else if err := applyProjectConfig(&c, b); err != nil {
			return c, fmt.Errorf("parse project config %s: %w", projectPath, err)
		}
	}
	applyEnv(&c)
	if err := c.Validate(); err != nil {
		return c, err
	}
	return c, nil
}

func applyProjectConfig(c *Config, b []byte) error {
	var p projectConfig
	if err := toml.NewDecoder(bytes.NewReader(b)).DisallowUnknownFields().Decode(&p); err != nil {
		return err
	}
	if p.Version != nil && *p.Version != 1 {
		return fmt.Errorf("unsupported config version %d (expected 1)", *p.Version)
	}
	if p.Provider != nil {
		c.Provider = *p.Provider
	}
	if p.Output != nil {
		if p.Output.Format != nil {
			c.Output.Format = *p.Output.Format
		}
	}
	if p.Upload != nil {
		if p.Upload.Path != nil {
			c.Upload.Path = *p.Upload.Path
		}
		if p.Upload.PathTemplate != nil {
			c.Upload.PathTemplate = *p.Upload.PathTemplate
		}
	}
	return nil
}

func ResolveProvider(p ProviderConfig) (ProviderConfig, error) {
	if err := expandCredentials(reflect.ValueOf(&p).Elem(), credential.Environment{}); err != nil {
		return p, err
	}
	return p, nil
}

func (c Config) Validate() error {
	if c.Version != 1 {
		return fmt.Errorf("unsupported config version %d (expected 1)", c.Version)
	}
	if c.Upload.Concurrency < 1 {
		return errors.New("upload.concurrency must be at least 1")
	}
	if c.Upload.MaxSize <= 0 {
		return errors.New("upload.max_size must be positive")
	}
	if c.Output.Format != "url" && c.Output.Format != "markdown" && c.Output.Format != "html" && c.Output.Format != "json" {
		return fmt.Errorf("unknown output format %q", c.Output.Format)
	}
	switch c.Upload.Rename {
	case "original", "timestamp", "hash", "uuid":
	default:
		return fmt.Errorf("unknown upload.rename %q", c.Upload.Rename)
	}
	switch c.Upload.Conflict {
	case "error", "overwrite":
	default:
		return fmt.Errorf("unknown or unsupported upload.conflict %q", c.Upload.Conflict)
	}
	for n, p := range c.Providers {
		switch p.Type {
		case "http":
			if p.URL == "" || p.URLJSONPath == "" {
				return fmt.Errorf("http provider %q requires url and url_json_path", n)
			}
			if err := validateNetworkURL(p.URL, p.AllowInsecure); err != nil {
				return fmt.Errorf("http provider %q: %w", n, err)
			}
		case "s3":
			if p.Bucket == "" || p.PublicURL == "" {
				return fmt.Errorf("s3 provider %q requires bucket and public_url", n)
			}
			if (p.AccessKey == "") != (p.SecretKey == "") {
				return fmt.Errorf("s3 provider %q requires both access_key and secret_key", n)
			}
			if p.Endpoint != "" {
				if err := validateNetworkURL(p.Endpoint, p.AllowInsecure); err != nil {
					return fmt.Errorf("s3 provider %q endpoint: %w", n, err)
				}
			}
			if err := validateNetworkURL(p.PublicURL, p.AllowInsecure); err != nil {
				return fmt.Errorf("s3 provider %q public_url: %w", n, err)
			}
		case "github":
			if p.Owner == "" || p.Repo == "" || p.Token == "" {
				return fmt.Errorf("github provider %q requires owner, repo, and token", n)
			}
			if p.PublicURL != "" {
				if err := validateNetworkURL(p.PublicURL, p.AllowInsecure); err != nil {
					return fmt.Errorf("github provider %q public_url: %w", n, err)
				}
			}
		default:
			return fmt.Errorf("provider %q has unknown type %q", n, p.Type)
		}
		if !c.AllowPlaintextCredentials {
			for key, value := range map[string]string{"access_key": p.AccessKey, "secret_key": p.SecretKey, "session_token": p.SessionToken, "token": p.Token} {
				if value != "" && !credential.IsReference(value) {
					return fmt.Errorf("provider %q contains plaintext %s; use an environment reference or explicitly set allow_plaintext_credentials = true", n, key)
				}
			}
			for key, value := range p.Headers {
				if isSensitive(key) && value != "" && !credential.IsReference(value) {
					return fmt.Errorf("provider %q contains plaintext sensitive header %q", n, key)
				}
			}
			for key, value := range p.Fields {
				if isSensitive(key) && value != "" && !credential.IsReference(value) {
					return fmt.Errorf("provider %q contains plaintext sensitive field %q", n, key)
				}
			}
		}
	}
	if c.DefaultProvider != "" {
		if _, ok := c.Providers[c.DefaultProvider]; !ok {
			return fmt.Errorf("default provider %q is not configured", c.DefaultProvider)
		}
	}
	if c.Provider != "" {
		if _, ok := c.Providers[c.Provider]; !ok {
			return fmt.Errorf("project provider %q is not configured globally", c.Provider)
		}
	}
	return nil
}

func validateNetworkURL(raw string, allowInsecure bool) error {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" {
		return fmt.Errorf("invalid absolute URL")
	}
	if u.User != nil || u.Fragment != "" {
		return fmt.Errorf("URL must not contain user information or a fragment")
	}
	if u.Scheme != "https" && !(allowInsecure && u.Scheme == "http") {
		return fmt.Errorf("URL must use HTTPS (set allow_insecure = true only for trusted local endpoints)")
	}
	return nil
}

func Save(path string, c Config) error {
	b, err := toml.Marshal(c)
	if err != nil {
		return fmt.Errorf("encode config: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return fmt.Errorf("create config directory: %w", err)
	}
	f, err := os.CreateTemp(filepath.Dir(path), ".config-*.tmp")
	if err != nil {
		return fmt.Errorf("open config for writing: %w", err)
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	if err := f.Chmod(0600); err != nil {
		_ = f.Close()
		return fmt.Errorf("secure config permissions: %w", err)
	}
	if _, err := f.Write(b); err != nil {
		_ = f.Close()
		return fmt.Errorf("write config: %w", err)
	}
	if err := f.Sync(); err != nil {
		_ = f.Close()
		return fmt.Errorf("sync config: %w", err)
	}
	if err := f.Close(); err != nil {
		return fmt.Errorf("write config: %w", err)
	}
	if err := os.Rename(tmp, path); err != nil {
		return fmt.Errorf("replace config atomically: %w", err)
	}
	return nil
}

func expandCredentials(v reflect.Value, resolver credential.Resolver) error {
	switch v.Kind() {
	case reflect.Struct:
		for i := 0; i < v.NumField(); i++ {
			if err := expandCredentials(v.Field(i), resolver); err != nil {
				return err
			}
		}
	case reflect.Map:
		for _, k := range v.MapKeys() {
			x := reflect.New(v.Type().Elem()).Elem()
			x.Set(v.MapIndex(k))
			if err := expandCredentials(x, resolver); err != nil {
				return err
			}
			v.SetMapIndex(k, x)
		}
	case reflect.String:
		val, err := resolver.Resolve(v.String())
		if err != nil {
			return err
		}
		v.SetString(val)
	}
	return nil
}

func applyEnv(c *Config) {
	if v := os.Getenv("IMG_PROVIDER"); v != "" {
		c.Provider = v
	}
	if v := os.Getenv("IMG_DEFAULT_PROVIDER"); v != "" {
		c.DefaultProvider = v
	}
	if v := os.Getenv("IMG_OUTPUT_FORMAT"); v != "" {
		c.Output.Format = v
	}
	if v := os.Getenv("IMG_OUTPUT_COPY"); v != "" {
		if b, e := strconv.ParseBool(v); e == nil {
			c.Output.Copy = b
		}
	}
	if v := os.Getenv("IMG_UPLOAD_CONCURRENCY"); v != "" {
		if n, e := strconv.Atoi(v); e == nil {
			c.Upload.Concurrency = n
		}
	}
}

func isSensitive(s string) bool {
	s = strings.NewReplacer("-", "", "_", "", " ", "").Replace(strings.ToLower(s))
	for _, k := range []string{"token", "secret", "password", "authorization", "apikey"} {
		if strings.Contains(s, k) {
			return true
		}
	}
	return false
}

func Redact(c Config) Config {
	for n, p := range c.Providers {
		p.AccessKey = mask(p.AccessKey, 4)
		p.SecretKey = mask(p.SecretKey, 0)
		p.SessionToken = mask(p.SessionToken, 0)
		p.Token = mask(p.Token, 4)
		for k, v := range p.Headers {
			if isSensitive(k) {
				p.Headers[k] = mask(v, 0)
			}
		}
		for k, v := range p.Fields {
			if isSensitive(k) {
				p.Fields[k] = mask(v, 0)
			}
		}
		c.Providers[n] = p
	}
	return c
}
func mask(v string, keep int) string {
	if v == "" {
		return ""
	}
	if keep > len(v) {
		keep = len(v)
	}
	return v[:keep] + strings.Repeat("*", max(8, len(v)-keep))
}
