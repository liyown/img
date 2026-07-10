package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"

	"github.com/liyown/img/internal/credential"
	"github.com/pelletier/go-toml/v2"
)

type Config struct {
	Version         int                       `toml:"version" json:"version"`
	DefaultProvider string                    `toml:"default_provider" json:"default_provider"`
	Provider        string                    `toml:"provider,omitempty" json:"provider,omitempty"`
	Output          Output                    `toml:"output" json:"output"`
	Upload          Upload                    `toml:"upload" json:"upload"`
	Providers       map[string]ProviderConfig `toml:"providers" json:"providers"`
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
	for _, p := range []string{globalPath, projectPath} {
		if p == "" {
			continue
		}
		b, err := os.ReadFile(p)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return c, fmt.Errorf("read config %s: %w", p, err)
		}
		if err := toml.Unmarshal(b, &c); err != nil {
			return c, fmt.Errorf("parse config %s: %w", p, err)
		}
	}
	applyEnv(&c)
	// Keep unresolved references intact. Only the selected provider resolves its
	// credentials strictly, so unrelated providers can use independent secrets.
	if err := expandCredentials(reflect.ValueOf(&c).Elem(), credential.OptionalEnvironment{}); err != nil {
		return c, err
	}
	if err := c.Validate(); err != nil {
		return c, err
	}
	return c, nil
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
	for n, p := range c.Providers {
		switch p.Type {
		case "http", "s3", "github":
		default:
			return fmt.Errorf("provider %q has unknown type %q", n, p.Type)
		}
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
	if err := os.WriteFile(path, b, 0600); err != nil {
		return fmt.Errorf("write config: %w", err)
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
	s = strings.ToLower(s)
	for _, k := range []string{"token", "secret", "secret_key", "access_token", "password", "authorization", "api_key"} {
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
