package httpprovider

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
)

const maxResponse = 1 << 20

type Provider struct {
	name   string
	cfg    config.ProviderConfig
	client *http.Client
}

func New(name string, c config.ProviderConfig, client *http.Client) *Provider {
	if client == nil {
		client = &http.Client{Timeout: 60 * time.Second}
	}
	return &Provider{name: name, cfg: c, client: client}
}
func (p *Provider) Name() string { return p.name }
func (p *Provider) Type() string { return "http" }
func (p *Provider) Validate(context.Context) error {
	if p.cfg.URL == "" {
		return fmt.Errorf("http provider %q: url is required", p.name)
	}
	if p.cfg.URLJSONPath == "" {
		return fmt.Errorf("http provider %q: url_json_path is required", p.name)
	}
	return nil
}
func (p *Provider) Upload(ctx context.Context, r model.UploadRequest) (*model.UploadResult, error) {
	if err := p.Validate(ctx); err != nil {
		return nil, err
	}
	f, err := os.Open(r.LocalPath)
	if err != nil {
		return nil, fmt.Errorf("open upload file: %w", err)
	}
	pr, pw := io.Pipe()
	mw := multipart.NewWriter(pw)
	go func() {
		defer f.Close()
		defer pw.Close()
		part, e := mw.CreateFormFile(value(p.cfg.FileField, "file"), filepath.Base(r.LocalPath))
		if e == nil {
			_, e = io.Copy(part, f)
		}
		if e == nil {
			for k, v := range p.cfg.Fields {
				e = mw.WriteField(k, v)
				if e != nil {
					break
				}
			}
		}
		if e == nil {
			e = mw.Close()
		}
		if e != nil {
			_ = pw.CloseWithError(e)
		}
	}()
	method := value(strings.ToUpper(p.cfg.Method), http.MethodPost)
	req, err := http.NewRequestWithContext(ctx, method, p.cfg.URL, pr)
	if err != nil {
		return nil, fmt.Errorf("build HTTP upload request: %w", err)
	}
	req.Header.Set("Content-Type", mw.FormDataContentType())
	for k, v := range p.cfg.Headers {
		req.Header.Set(k, v)
	}
	resp, err := p.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("HTTP upload to provider %q: %w", p.name, err)
	}
	defer resp.Body.Close()
	b, err := io.ReadAll(io.LimitReader(resp.Body, maxResponse+1))
	if err != nil {
		return nil, fmt.Errorf("read HTTP upload response: %w", err)
	}
	if len(b) > maxResponse {
		return nil, fmt.Errorf("HTTP upload response exceeds %d bytes", maxResponse)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("HTTP upload failed with status %d: %s", resp.StatusCode, p.sanitize(truncate(string(b), 512)))
	}
	var doc any
	if err := json.Unmarshal(b, &doc); err != nil {
		return nil, fmt.Errorf("decode HTTP upload response: %w", err)
	}
	u, err := jsonPath(doc, p.cfg.URLJSONPath)
	if err != nil {
		return nil, err
	}
	st, _ := os.Stat(r.LocalPath)
	return &model.UploadResult{Provider: p.name, LocalPath: r.LocalPath, RemotePath: r.RemotePath, URL: u, Size: st.Size(), ContentType: r.ContentType}, nil
}

func (p *Provider) sanitize(s string) string {
	for k, v := range p.cfg.Headers {
		lk := strings.ToLower(k)
		if v != "" && (strings.Contains(lk, "authorization") || strings.Contains(lk, "token") || strings.Contains(lk, "secret") || strings.Contains(lk, "password") || strings.Contains(lk, "api-key")) {
			s = strings.ReplaceAll(s, v, "********")
		}
	}
	return s
}
func jsonPath(v any, p string) (string, error) {
	cur := v
	for _, key := range strings.Split(p, ".") {
		m, ok := cur.(map[string]any)
		if !ok {
			return "", fmt.Errorf("JSON path %q traverses a non-object at %q", p, key)
		}
		cur, ok = m[key]
		if !ok {
			return "", fmt.Errorf("JSON path %q not found", p)
		}
	}
	s, ok := cur.(string)
	if !ok || s == "" {
		return "", fmt.Errorf("JSON path %q is not a non-empty string", p)
	}
	return s, nil
}
func value(v, d string) string {
	if v == "" {
		return d
	}
	return v
}
func truncate(s string, n int) string {
	if len(s) > n {
		return s[:n] + "…"
	}
	return s
}
