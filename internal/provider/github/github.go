package githubprovider

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	"github.com/liyown/img/internal/pathgen"
)

type Provider struct {
	name   string
	cfg    config.ProviderConfig
	client *http.Client
	api    string
}

func New(name string, c config.ProviderConfig, client *http.Client) *Provider {
	if client == nil {
		client = &http.Client{Timeout: 60 * time.Second}
	}
	return &Provider{name: name, cfg: c, client: client, api: "https://api.github.com"}
}
func NewWithAPI(name string, c config.ProviderConfig, client *http.Client, api string) *Provider {
	p := New(name, c, client)
	p.api = api
	return p
}
func (p *Provider) Name() string { return p.name }
func (p *Provider) Type() string { return "github" }
func (p *Provider) Validate(context.Context) error {
	if p.cfg.Owner == "" || p.cfg.Repo == "" || p.cfg.Token == "" {
		return fmt.Errorf("github provider %q: owner, repo, and token are required", p.name)
	}
	return nil
}
func (p *Provider) Test(ctx context.Context) error {
	if err := p.Validate(ctx); err != nil {
		return err
	}
	endpoint := fmt.Sprintf("%s/repos/%s/%s", strings.TrimRight(p.api, "/"), url.PathEscape(p.cfg.Owner), url.PathEscape(p.cfg.Repo))
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return fmt.Errorf("build GitHub provider test: %w", err)
	}
	p.auth(req)
	resp, err := p.client.Do(req)
	if err != nil {
		return fmt.Errorf("test GitHub provider %q: %w", p.name, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
		return fmt.Errorf("test GitHub provider %q failed with status %d: %s", p.name, resp.StatusCode, p.sanitize(string(b)))
	}
	return nil
}
func (p *Provider) Upload(ctx context.Context, r model.UploadRequest) (*model.UploadResult, error) {
	if err := p.Validate(ctx); err != nil {
		return nil, err
	}
	branch := p.cfg.Branch
	if branch == "" {
		branch = "main"
	}
	endpoint := fmt.Sprintf("%s/repos/%s/%s/contents/%s", strings.TrimRight(p.api, "/"), url.PathEscape(p.cfg.Owner), url.PathEscape(p.cfg.Repo), pathgen.EscapeURLPath(r.RemotePath))
	sha, exists, err := p.lookup(ctx, endpoint, branch)
	if err != nil {
		return nil, err
	}
	if exists && !r.Overwrite {
		return nil, fmt.Errorf("GitHub file %q already exists; use --overwrite", r.RemotePath)
	}
	if r.Body == nil {
		return nil, fmt.Errorf("upload body is required")
	}
	if _, err := r.Body.Seek(0, io.SeekStart); err != nil {
		return nil, fmt.Errorf("rewind upload body: %w", err)
	}
	b, err := io.ReadAll(io.LimitReader(r.Body, r.Size+1))
	if err != nil {
		return nil, fmt.Errorf("read upload body: %w", err)
	}
	if int64(len(b)) != r.Size {
		return nil, fmt.Errorf("upload body size changed during upload")
	}
	msg := p.cfg.CommitMessage
	if msg == "" {
		msg = "upload: {path}"
	}
	msg = strings.ReplaceAll(msg, "{path}", r.RemotePath)
	payload := map[string]any{"message": msg, "content": base64.StdEncoding.EncodeToString(b), "branch": branch}
	if sha != "" {
		payload["sha"] = sha
	}
	body, _ := json.Marshal(payload)
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, endpoint, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("build GitHub request: %w", err)
	}
	p.auth(req)
	req.Header.Set("Content-Type", "application/json")
	resp, err := p.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("GitHub upload: %w", err)
	}
	defer resp.Body.Close()
	rb, _ := io.ReadAll(io.LimitReader(resp.Body, 64<<10))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 403 && resp.Header.Get("X-RateLimit-Remaining") == "0" {
			return nil, fmt.Errorf("GitHub API rate limit exceeded")
		}
		return nil, fmt.Errorf("GitHub upload failed with status %d: %s", resp.StatusCode, p.sanitize(string(rb)))
	}
	public := p.cfg.PublicURL
	if public == "" {
		public = fmt.Sprintf("https://raw.githubusercontent.com/%s/%s/%s", p.cfg.Owner, p.cfg.Repo, branch)
	}
	return &model.UploadResult{Provider: p.name, LocalPath: r.LocalPath, RemotePath: r.RemotePath, URL: strings.TrimRight(public, "/") + "/" + pathgen.EscapeURLPath(r.RemotePath), Size: int64(len(b)), ContentType: r.ContentType}, nil
}
func (p *Provider) lookup(ctx context.Context, endpoint, branch string) (string, bool, error) {
	req, _ := http.NewRequestWithContext(ctx, http.MethodGet, endpoint+"?ref="+url.QueryEscape(branch), nil)
	p.auth(req)
	resp, err := p.client.Do(req)
	if err != nil {
		return "", false, fmt.Errorf("check GitHub file: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == 404 {
		return "", false, nil
	}
	b, _ := io.ReadAll(io.LimitReader(resp.Body, 64<<10))
	if resp.StatusCode != 200 {
		if resp.StatusCode == 403 && resp.Header.Get("X-RateLimit-Remaining") == "0" {
			return "", false, fmt.Errorf("GitHub API rate limit exceeded")
		}
		return "", false, fmt.Errorf("check GitHub file failed with status %d: %s", resp.StatusCode, p.sanitize(string(b)))
	}
	var v struct {
		SHA string `json:"sha"`
	}
	if err := json.Unmarshal(b, &v); err != nil {
		return "", false, fmt.Errorf("decode GitHub file response: %w", err)
	}
	return v.SHA, true, nil
}
func (p *Provider) sanitize(s string) string {
	if p.cfg.Token != "" {
		s = strings.ReplaceAll(s, p.cfg.Token, "********")
	}
	return strings.Map(func(r rune) rune {
		if r < 32 || r == 127 {
			return -1
		}
		return r
	}, s)
}
func (p *Provider) auth(r *http.Request) {
	r.Header.Set("Authorization", "Bearer "+p.cfg.Token)
	r.Header.Set("Accept", "application/vnd.github+json")
	r.Header.Set("X-GitHub-Api-Version", "2022-11-28")
}
