package httpprovider

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/url"
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
	secured := *client
	originalRedirect := client.CheckRedirect
	secured.CheckRedirect = func(req *http.Request, via []*http.Request) error {
		if len(via) >= 5 {
			return fmt.Errorf("too many HTTP redirects")
		}
		if len(via) > 0 && (req.URL.Scheme != via[0].URL.Scheme || !strings.EqualFold(req.URL.Host, via[0].URL.Host)) {
			return fmt.Errorf("refusing cross-origin redirect from %s to %s", via[0].URL.Host, req.URL.Host)
		}
		if originalRedirect != nil {
			return originalRedirect(req, via)
		}
		return nil
	}
	return &Provider{name: name, cfg: c, client: &secured}
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
	if _, err := secureURL(p.cfg.URL, p.cfg.AllowInsecure); err != nil {
		return fmt.Errorf("http provider %q: %w", p.name, err)
	}
	method := strings.ToUpper(value(p.cfg.Method, http.MethodPost))
	if method != http.MethodPost && method != http.MethodPut && method != http.MethodPatch {
		return fmt.Errorf("http provider %q: unsupported upload method %q", p.name, method)
	}
	return nil
}
func (p *Provider) Test(ctx context.Context) error {
	if err := p.Validate(ctx); err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodHead, p.cfg.URL, nil)
	if err != nil {
		return fmt.Errorf("build HTTP provider test: %w", err)
	}
	for k, v := range p.cfg.Headers {
		req.Header.Set(k, v)
	}
	resp, err := p.client.Do(req)
	if err != nil {
		return fmt.Errorf("test HTTP provider %q: %w", p.name, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
		return fmt.Errorf("test HTTP provider %q: status %d", p.name, resp.StatusCode)
	}
	if resp.StatusCode >= 500 {
		return fmt.Errorf("test HTTP provider %q: status %d", p.name, resp.StatusCode)
	}
	return nil
}
func (p *Provider) Upload(ctx context.Context, r model.UploadRequest) (*model.UploadResult, error) {
	if err := p.Validate(ctx); err != nil {
		return nil, err
	}
	if r.Body == nil {
		return nil, fmt.Errorf("upload body is required")
	}
	pr, pw := io.Pipe()
	mw := multipart.NewWriter(pw)
	method := value(strings.ToUpper(p.cfg.Method), http.MethodPost)
	req, err := http.NewRequestWithContext(ctx, method, p.cfg.URL, pr)
	if err != nil {
		_ = pr.Close()
		_ = pw.Close()
		return nil, fmt.Errorf("build HTTP upload request: %w", err)
	}
	req.Header.Set("Content-Type", mw.FormDataContentType())
	for k, v := range p.cfg.Headers {
		req.Header.Set(k, v)
	}
	go func() {
		defer pw.Close()
		part, e := mw.CreateFormFile(value(p.cfg.FileField, "file"), r.FileName)
		if e == nil {
			var n int64
			n, e = io.Copy(part, r.Body)
			if e == nil && n != r.Size {
				e = fmt.Errorf("upload body size changed: expected %d bytes, read %d", r.Size, n)
			}
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
	if _, err := secureURL(u, p.cfg.AllowInsecure); err != nil {
		return nil, fmt.Errorf("HTTP upload returned an unsafe URL: %w", err)
	}
	return &model.UploadResult{Provider: p.name, LocalPath: r.LocalPath, RemotePath: r.RemotePath, URL: u, Size: r.Size, ContentType: r.ContentType}, nil
}

func (p *Provider) sanitize(s string) string {
	for k, v := range p.cfg.Headers {
		lk := strings.ToLower(k)
		if v != "" && (strings.Contains(lk, "authorization") || strings.Contains(lk, "token") || strings.Contains(lk, "secret") || strings.Contains(lk, "password") || strings.Contains(lk, "api-key")) {
			s = redactValue(s, v)
		}
	}
	for k, v := range p.cfg.Fields {
		lk := strings.NewReplacer("-", "", "_", "").Replace(strings.ToLower(k))
		if strings.Contains(lk, "token") || strings.Contains(lk, "secret") || strings.Contains(lk, "password") || strings.Contains(lk, "apikey") {
			s = redactValue(s, v)
		}
	}
	return stripControls(s)
}
func redactValue(s, v string) string {
	if v == "" {
		return s
	}
	s = strings.ReplaceAll(s, v, "********")
	parts := strings.Fields(v)
	if len(parts) > 1 {
		for _, part := range parts[1:] {
			if len(part) >= 4 {
				s = strings.ReplaceAll(s, part, "********")
			}
		}
	}
	return s
}
func secureURL(raw string, allowInsecure bool) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" {
		return nil, fmt.Errorf("URL must be absolute")
	}
	if u.User != nil || u.Fragment != "" {
		return nil, fmt.Errorf("URL must not contain user information or a fragment")
	}
	if u.Scheme != "https" && !(allowInsecure && u.Scheme == "http") {
		return nil, fmt.Errorf("URL must use HTTPS")
	}
	for _, r := range raw {
		if r < 32 || r == 127 {
			return nil, fmt.Errorf("URL contains a control character")
		}
	}
	return u, nil
}
func stripControls(s string) string {
	return strings.Map(func(r rune) rune {
		if r < 32 || r == 127 {
			return -1
		}
		return r
	}, s)
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
