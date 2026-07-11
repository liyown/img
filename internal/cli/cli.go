package cli

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"text/tabwriter"

	"github.com/liyown/img/internal/clipboard"
	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/fetch"
	"github.com/liyown/img/internal/mdimg"
	"github.com/liyown/img/internal/model"
	"github.com/liyown/img/internal/output"
	"github.com/liyown/img/internal/provider"
	"github.com/liyown/img/internal/screenshot"
	"github.com/liyown/img/internal/serve"
	"github.com/liyown/img/internal/upload"
	"github.com/pelletier/go-toml/v2"
)

var Version = "dev"
var Commit = "unknown"
var BuildDate = "unknown"

type CLI struct {
	Out, Err   io.Writer
	In         io.Reader
	Clipboard  clipboard.Clipboard
	GlobalPath string
}

func New() *CLI {
	p, _ := config.GlobalPath()
	return &CLI{Out: os.Stdout, Err: os.Stderr, In: os.Stdin, Clipboard: clipboard.System{}, GlobalPath: p}
}
func (c *CLI) Run(ctx context.Context, args []string) int {
	if len(args) == 0 {
		c.usage()
		return 2
	}
	cmd := args[0]
	rest := args[1:]
	if !known(cmd) {
		cmd = "upload"
		rest = args
	}
	var err error
	switch cmd {
	case "upload":
		return c.upload(ctx, rest)
	case "rewrite":
		return c.rewrite(ctx, rest)
	case "screenshot":
		return c.screenshot(ctx, rest)
	case "serve":
		return c.serveCmd(ctx, rest)
	case "init":
		err = c.init(rest)
	case "provider":
		err = c.provider(ctx, rest)
	case "config":
		err = c.config(rest)
	case "version":
		fmt.Fprintf(c.Out, "img %s\ncommit: %s\nbuilt: %s\n", Version, Commit, BuildDate)
		return 0
	case "help", "--help", "-h":
		c.usage()
		return 0
	}
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	return 0
}
func known(s string) bool {
	switch s {
	case "upload", "init", "provider", "config", "version",
		"rewrite", "screenshot", "serve", "help", "--help", "-h":
		return true
	}
	return false
}
func (c *CLI) load() (config.Config, error) {
	return config.Load(c.GlobalPath, filepath.Join(mustwd(), ".img.toml"))
}
func (c *CLI) upload(ctx context.Context, args []string) int {
	fs := flag.NewFlagSet("upload", flag.ContinueOnError)
	fs.SetOutput(c.Err)
	var pn, format, path, name string
	var copyFlag, noCopy, overwrite, verbose, quietFlag, optimizeFlag, allowInsecure bool
	fs.StringVar(&pn, "provider", "", "provider name")
	fs.StringVar(&format, "format", "", "url, markdown, html, or json")
	fs.BoolVar(&copyFlag, "copy", false, "copy output")
	fs.BoolVar(&noCopy, "no-copy", false, "do not copy output")
	fs.BoolVar(&quietFlag, "quiet", false, "suppress stdout output (useful with --copy)")
	fs.StringVar(&path, "path", "", "remote path prefix")
	fs.StringVar(&name, "name", "", "remote file name")
	fs.BoolVar(&overwrite, "overwrite", false, "overwrite existing object")
	fs.BoolVar(&verbose, "verbose", false, "verbose logging")
	fs.BoolVar(&optimizeFlag, "optimize", false, "compress images before upload (JPEG→q85, opaque PNG→JPEG)")
	fs.BoolVar(&allowInsecure, "allow-insecure", false, "allow fetching source URLs over plain HTTP")
	ordered, err := reorder(args, map[string]bool{"--provider": true, "--format": true, "--path": true, "--name": true})
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if err = fs.Parse(ordered); err != nil {
		return 2
	}
	files := fs.Args()
	if len(files) == 0 {
		fmt.Fprintln(c.Err, "Error: at least one image file is required")
		return 2
	}
	if name != "" && len(files) > 1 {
		fmt.Fprintln(c.Err, "Error: --name can only be used with one file")
		return 2
	}
	cfg, err := c.load()
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if pn == "" {
		pn = cfg.Provider
		if pn == "" {
			pn = cfg.DefaultProvider
		}
	}
	if pn == "" {
		fmt.Fprintln(c.Err, "Error: no provider selected; run img init")
		return 2
	}
	pc, ok := cfg.Providers[pn]
	if !ok {
		fmt.Fprintf(c.Err, "Error: provider %q is not configured\n", pn)
		return 2
	}
	if format == "" {
		format = cfg.Output.Format
	}
	if !validFormat(format) {
		fmt.Fprintf(c.Err, "Error: unknown output format %q\n", format)
		return 2
	}
	if verbose {
		fmt.Fprintf(c.Err, "Using provider %s (%s)\n", pn, pc.Type)
	}
	p, err := provider.New(ctx, pn, pc)
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if err = p.Validate(ctx); err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	results := upload.Run(ctx, p, cfg.Upload, files, upload.Options{Path: path, Name: name, Overwrite: overwrite, Optimize: optimizeFlag, AllowInsecure: allowInsecure})
	// In verbose mode, report compression savings for each optimised file.
	if verbose && optimizeFlag {
		for _, r := range results {
			if r.Success && r.OriginalSize > 0 {
				saved := 100 - int(r.Size*100/r.OriginalSize)
				fmt.Fprintf(c.Err, "Optimized %s: %s → %s (−%d%%)\n",
					r.LocalPath, formatBytes(r.OriginalSize), formatBytes(r.Size), saved)
			}
		}
	}
	// Quiet mode: --quiet flag or output.quiet config suppresses stdout output.
	// JSON output is also silenced in quiet mode; use --copy to get it on clipboard.
	doQuiet := cfg.Output.Quiet || quietFlag
	if !doQuiet {
		_ = output.Render(c.Out, format, results)
	}
	doCopy := cfg.Output.Copy
	if copyFlag {
		doCopy = true
	}
	if noCopy {
		doCopy = false
	}
	if doCopy {
		if err := c.Clipboard.Write(output.ClipboardText(format, results)); err != nil {
			fmt.Fprintln(c.Err, "Warning: upload succeeded, but clipboard copy failed:", err)
		}
	}
	failed := 0
	for _, r := range results {
		if !r.Success {
			failed++
		}
	}
	if failed == 0 {
		return 0
	}
	if failed < len(results) {
		return 3
	}
	return 1
}

// rewrite downloads or uploads every image referenced in a Markdown article
// and rewrites the document with the new image-host URLs.
//
// Usage:
//
//	img rewrite article.md [options]   # rewrite file in-place
//	img rewrite article.md --stdout    # print result to stdout
//	cat article.md | img rewrite       # read stdin → stdout
func (c *CLI) rewrite(ctx context.Context, args []string) int {
	fs := flag.NewFlagSet("rewrite", flag.ContinueOnError)
	fs.SetOutput(c.Err)
	var pn, path string
	var overwrite, optimize, allowInsecure, toStdout bool
	fs.StringVar(&pn, "provider", "", "provider name")
	fs.StringVar(&path, "path", "", "remote path prefix")
	fs.BoolVar(&overwrite, "overwrite", false, "overwrite existing objects")
	fs.BoolVar(&optimize, "optimize", false, "compress images before upload")
	fs.BoolVar(&allowInsecure, "allow-insecure", false, "allow fetching source URLs over plain HTTP")
	fs.BoolVar(&toStdout, "stdout", false, "write result to stdout instead of rewriting the file")
	ordered, err := reorder(args, map[string]bool{"--provider": true, "--path": true})
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if err = fs.Parse(ordered); err != nil {
		return 2
	}

	// ── Read article ──────────────────────────────────────────────────────
	fileArg := ""
	if rest := fs.Args(); len(rest) > 0 {
		fileArg = rest[0]
	}
	var articleBytes []byte
	var articleDir string
	if fileArg != "" {
		articleBytes, err = os.ReadFile(fileArg)
		if err != nil {
			fmt.Fprintln(c.Err, "Error:", err)
			return 2
		}
		if abs, e := filepath.Abs(filepath.Dir(fileArg)); e == nil {
			articleDir = abs
		} else {
			articleDir = filepath.Dir(fileArg)
		}
	} else {
		articleBytes, err = io.ReadAll(c.In)
		if err != nil {
			fmt.Fprintln(c.Err, "Error: read stdin:", err)
			return 2
		}
		articleDir = mustwd()
		toStdout = true // no file to write back to
	}

	// ── Extract image references ──────────────────────────────────────────
	doc := string(articleBytes)
	refs := mdimg.Extract(doc)
	if len(refs) == 0 {
		return c.writeRewritten(fileArg, toStdout, articleBytes)
	}

	// ── Resolve provider ─────────────────────────────────────────────────
	cfg, err := c.load()
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if pn == "" {
		pn = cfg.Provider
		if pn == "" {
			pn = cfg.DefaultProvider
		}
	}
	if pn == "" {
		fmt.Fprintln(c.Err, "Error: no provider selected; run img init")
		return 2
	}
	pc, ok := cfg.Providers[pn]
	if !ok {
		fmt.Fprintf(c.Err, "Error: provider %q is not configured\n", pn)
		return 2
	}
	p, err := provider.New(ctx, pn, pc)
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if err = p.Validate(ctx); err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}

	// ── Build upload source list ──────────────────────────────────────────
	// Local relative paths are resolved to absolute so upload.one can open
	// them; we remember the original ref string for text replacement later.
	type entry struct {
		original string // as it appears in the document
		source   string // passed to upload.Run (abs path or URL)
	}
	entries := make([]entry, len(refs))
	sources := make([]string, len(refs))
	for i, r := range refs {
		src := r.Src
		if !fetch.IsURL(src) && !filepath.IsAbs(src) {
			src = filepath.Join(articleDir, src)
		}
		entries[i] = entry{original: r.Src, source: src}
		sources[i] = src
	}

	// ── Upload ───────────────────────────────────────────────────────────
	results := upload.Run(ctx, p, cfg.Upload, sources, upload.Options{
		Path:          path,
		Overwrite:     overwrite,
		Optimize:      optimize,
		AllowInsecure: allowInsecure,
	})

	// ── Build replacement map and report ─────────────────────────────────
	replacements := make(map[string]string, len(results))
	succeeded, failed := 0, 0
	for i, r := range results {
		orig := entries[i].original
		if r.Success {
			replacements[orig] = r.URL
			succeeded++
		} else {
			fmt.Fprintf(c.Err, "Warning: %s: %s (kept original)\n",
				cleanOutput(orig), cleanOutput(r.Error))
			failed++
		}
	}

	// ── Rewrite and output ────────────────────────────────────────────────
	out := mdimg.Rewrite(doc, replacements)
	code := c.writeRewritten(fileArg, toStdout, []byte(out))

	if fileArg != "" && !toStdout {
		fmt.Fprintf(c.Err, "Rewrite: %d uploaded, %d failed.\n", succeeded, failed)
	}
	if code != 0 {
		return code
	}
	if failed > 0 && succeeded > 0 {
		return 3
	}
	if failed > 0 {
		return 1
	}
	return 0
}

// writeRewritten writes data either to stdout or back to the source file using
// an atomic rename so a write failure never corrupts the original.
func (c *CLI) writeRewritten(fileArg string, toStdout bool, data []byte) int {
	if fileArg == "" || toStdout {
		if _, err := c.Out.Write(data); err != nil {
			fmt.Fprintln(c.Err, "Error: write output:", err)
			return 1
		}
		return 0
	}
	dir := filepath.Dir(fileArg)
	f, err := os.CreateTemp(dir, ".rewrite-*.tmp")
	if err != nil {
		fmt.Fprintln(c.Err, "Error: create temp file:", err)
		return 1
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	if _, err := f.Write(data); err != nil {
		f.Close()
		fmt.Fprintln(c.Err, "Error: write temp file:", err)
		return 1
	}
	if err := f.Sync(); err != nil {
		f.Close()
		fmt.Fprintln(c.Err, "Error: sync temp file:", err)
		return 1
	}
	if err := f.Close(); err != nil {
		fmt.Fprintln(c.Err, "Error: close temp file:", err)
		return 1
	}
	if err := os.Rename(tmp, fileArg); err != nil {
		fmt.Fprintln(c.Err, "Error: replace file:", err)
		return 1
	}
	return 0
}

// cleanOutput strips control characters from a string before printing to
// stderr, mirroring the Render package's cleanTerminal helper.
func cleanOutput(s string) string {
	return strings.Map(func(r rune) rune {
		if r < 32 && r != '\t' || r == 127 {
			return -1
		}
		return r
	}, s)
}

// screenshot captures a screenshot and uploads it in one step.
//
//	img screenshot                    # full screen, copy URL to clipboard
//	img screenshot --region           # interactive region selection
//	img screenshot --window           # active window
//	img screenshot --format markdown  # output Markdown link
func (c *CLI) screenshot(ctx context.Context, args []string) int {
	fs := flag.NewFlagSet("screenshot", flag.ContinueOnError)
	fs.SetOutput(c.Err)
	var pn, format, remotePath string
	var region, window, noCopy, optimize, verbose bool
	fs.StringVar(&pn, "provider", "", "provider name")
	fs.StringVar(&format, "format", "", "output format: url, markdown, html, json")
	fs.StringVar(&remotePath, "path", "", "remote path prefix")
	fs.BoolVar(&region, "region", false, "interactive region/area selection")
	fs.BoolVar(&window, "window", false, "capture the active window")
	fs.BoolVar(&noCopy, "no-copy", false, "do not copy result to clipboard")
	fs.BoolVar(&optimize, "optimize", false, "compress image before upload")
	fs.BoolVar(&verbose, "verbose", false, "verbose logging")
	if err := fs.Parse(args); err != nil {
		return 2
	}

	// Determine capture mode.
	mode := screenshot.ModeFullScreen
	switch {
	case region:
		mode = screenshot.ModeRegion
	case window:
		mode = screenshot.ModeWindow
	}

	tmpPath, err := screenshot.Capture(mode)
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	defer os.Remove(tmpPath)

	cfg, err := c.load()
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if pn == "" {
		pn = cfg.Provider
		if pn == "" {
			pn = cfg.DefaultProvider
		}
	}
	if pn == "" {
		fmt.Fprintln(c.Err, "Error: no provider selected; run img init")
		return 2
	}
	pc, ok := cfg.Providers[pn]
	if !ok {
		fmt.Fprintf(c.Err, "Error: provider %q is not configured\n", pn)
		return 2
	}
	if format == "" {
		format = cfg.Output.Format
	}
	if !validFormat(format) {
		fmt.Fprintf(c.Err, "Error: unknown output format %q\n", format)
		return 2
	}
	if verbose {
		fmt.Fprintf(c.Err, "Using provider %s (%s)\n", pn, pc.Type)
	}
	p, err := provider.New(ctx, pn, pc)
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if err = p.Validate(ctx); err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}

	results := upload.Run(ctx, p, cfg.Upload, []string{tmpPath},
		upload.Options{Path: remotePath, Optimize: optimize})

	doQuiet := cfg.Output.Quiet
	if !doQuiet {
		_ = output.Render(c.Out, format, results)
	}
	// Screenshot defaults to copying the result — skip only on --no-copy.
	doCopy := !noCopy
	if doCopy {
		if err := c.Clipboard.Write(output.ClipboardText(format, results)); err != nil {
			fmt.Fprintln(c.Err, "Warning: screenshot uploaded, but clipboard copy failed:", err)
		}
	}

	if verbose && results[0].OriginalSize > 0 {
		saved := 100 - int(results[0].Size*100/results[0].OriginalSize)
		fmt.Fprintf(c.Err, "Optimized: %s → %s (−%d%%)\n",
			formatBytes(results[0].OriginalSize), formatBytes(results[0].Size), saved)
	}
	if results[0].Success {
		return 0
	}
	return 1
}

// serveCmd runs a PicGo-compatible local HTTP server so any editor that
// supports a custom PicGo endpoint can upload images via img.
//
//	img serve                         # listen on 127.0.0.1:36677
//	img serve --port 9000             # custom port
//	img serve --bind 0.0.0.0          # all interfaces (use with care)
//	img serve --optimize              # compress images before upload
//
// Configure Typora: Preferences → Image → Image Uploader → Custom Command
//
//	img "${filepath}"
//
// Configure Obsidian (Image Auto Upload Plugin):
//
//	Upload Server: http://127.0.0.1:36677/upload
func (c *CLI) serveCmd(ctx context.Context, args []string) int {
	fs := flag.NewFlagSet("serve", flag.ContinueOnError)
	fs.SetOutput(c.Err)
	var pn, bind string
	var port int
	var optimize bool
	fs.StringVar(&pn, "provider", "", "provider name")
	fs.StringVar(&bind, "bind", "127.0.0.1", "address to bind")
	fs.IntVar(&port, "port", 36677, "port to listen on")
	fs.BoolVar(&optimize, "optimize", false, "compress images before upload")
	if err := fs.Parse(args); err != nil {
		return 2
	}

	cfg, err := c.load()
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if pn == "" {
		pn = cfg.Provider
		if pn == "" {
			pn = cfg.DefaultProvider
		}
	}
	if pn == "" {
		fmt.Fprintln(c.Err, "Error: no provider selected; run img init")
		return 2
	}
	pc, ok := cfg.Providers[pn]
	if !ok {
		fmt.Fprintf(c.Err, "Error: provider %q is not configured\n", pn)
		return 2
	}
	p, err := provider.New(ctx, pn, pc)
	if err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}
	if err = p.Validate(ctx); err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 2
	}

	addr := fmt.Sprintf("%s:%d", bind, port)
	s := &serve.Server{
		Provider: p,
		Cfg:      cfg.Upload,
		Opts:     serve.Options{UploadOpts: upload.Options{Optimize: optimize}},
		Out:      c.Out,
		Err:      c.Err,
	}
	if err := s.ListenAndServe(ctx, addr); err != nil {
		fmt.Fprintln(c.Err, "Error:", err)
		return 1
	}
	return 0
}

func (c *CLI) init(args []string) error {
	fs := flag.NewFlagSet("init", flag.ContinueOnError)
	fs.SetOutput(c.Err)
	var name, typ, url, jsonpath, endpoint, region, bucket, access, secret, public string
	var owner, repo, branch, token, commitMsg string
	var pathStyle, allowInsecure bool
	fs.StringVar(&name, "name", "", "provider name")
	fs.StringVar(&typ, "type", "", "http, s3, or github")
	fs.StringVar(&url, "url", "", "HTTP upload URL")
	fs.StringVar(&jsonpath, "url-json-path", "data.url", "JSON URL path")
	fs.StringVar(&endpoint, "endpoint", "", "S3 endpoint")
	fs.StringVar(&region, "region", "auto", "S3 region")
	fs.StringVar(&bucket, "bucket", "", "S3 bucket")
	fs.StringVar(&access, "access-key", "", "environment reference, e.g. ${IMG_ACCESS_KEY}")
	fs.StringVar(&secret, "secret-key", "", "environment reference, e.g. ${IMG_SECRET_KEY}")
	fs.StringVar(&public, "public-url", "", "public base URL")
	fs.BoolVar(&pathStyle, "path-style", false, "use S3 path style")
	fs.BoolVar(&allowInsecure, "allow-insecure", false, "allow trusted HTTP endpoints without TLS")
	fs.StringVar(&owner, "owner", "", "GitHub owner (user or org)")
	fs.StringVar(&repo, "repo", "", "GitHub repository name")
	fs.StringVar(&branch, "branch", "main", "GitHub branch")
	fs.StringVar(&token, "token", "", "environment reference, e.g. ${IMG_GITHUB_TOKEN}")
	fs.StringVar(&commitMsg, "commit-message", "upload: {path}", "GitHub commit message template")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if typ == "" {
		r := bufio.NewReader(c.In)
		fmt.Fprint(c.Out, "Provider type (http/s3/github): ")
		typ = strings.TrimSpace(readline(r))
		fmt.Fprint(c.Out, "Provider name: ")
		name = strings.TrimSpace(readline(r))
		switch typ {
		case "http":
			fmt.Fprint(c.Out, "Upload URL: ")
			url = strings.TrimSpace(readline(r))
		case "s3":
			fmt.Fprint(c.Out, "Bucket: ")
			bucket = strings.TrimSpace(readline(r))
			fmt.Fprint(c.Out, "Public URL: ")
			public = strings.TrimSpace(readline(r))
		case "github":
			fmt.Fprint(c.Out, "Owner: ")
			owner = strings.TrimSpace(readline(r))
			fmt.Fprint(c.Out, "Repository: ")
			repo = strings.TrimSpace(readline(r))
			fmt.Fprint(c.Out, "Token (env ref, e.g. ${IMG_GITHUB_TOKEN}): ")
			token = strings.TrimSpace(readline(r))
		}
	}
	if name == "" {
		name = typ
	}
	switch typ {
	case "http", "s3", "github":
	default:
		return fmt.Errorf("unsupported provider type %q (supported: http, s3, github)", typ)
	}
	cfg := config.Defaults()
	if b, e := os.ReadFile(c.GlobalPath); e == nil {
		if e = toml.Unmarshal(b, &cfg); e != nil {
			return fmt.Errorf("parse existing config: %w", e)
		}
	}
	pc := config.ProviderConfig{
		Type: typ,
		// http
		URL: url, Method: "POST", FileField: "file", URLJSONPath: jsonpath,
		AllowInsecure: allowInsecure,
		// s3
		Endpoint: endpoint, Region: region, Bucket: bucket,
		AccessKey: access, SecretKey: secret, PublicURL: public, PathStyle: pathStyle,
		// github
		Owner: owner, Repo: repo, Branch: branch, Token: token, CommitMessage: commitMsg,
	}
	cfg.Providers[name] = pc
	cfg.DefaultProvider = name
	if err := pcValidate(name, pc); err != nil {
		return err
	}
	if err := cfg.Validate(); err != nil {
		return err
	}
	if err := config.Save(c.GlobalPath, cfg); err != nil {
		return err
	}
	fmt.Fprintf(c.Out, "%s provider %q configured.\nDefault provider: %s\n", strings.ToUpper(typ), name, name)
	if typ == "github" && cfg.Upload.MaxSize > 1<<20 {
		fmt.Fprintf(c.Out, "Note: GitHub Contents API recommends files ≤ 1 MB. To enforce: img config set upload.max_size %d\n", 1<<20)
	}
	return nil
}
func (c *CLI) provider(ctx context.Context, args []string) error {
	if len(args) == 0 {
		return errors.New("provider subcommand required: list, show, use, remove, test")
	}
	cfg, err := c.load()
	if err != nil {
		return err
	}
	switch args[0] {
	case "list":
		tw := tabwriter.NewWriter(c.Out, 0, 0, 2, ' ', 0)
		fmt.Fprintln(tw, "NAME\tTYPE\tDEFAULT\tSTATUS")
		names := make([]string, 0, len(cfg.Providers))
		for n := range cfg.Providers {
			names = append(names, n)
		}
		sort.Strings(names)
		for _, n := range names {
			p := cfg.Providers[n]
			d := "no"
			if n == cfg.DefaultProvider {
				d = "yes"
			}
			fmt.Fprintf(tw, "%s\t%s\t%s\tconfigured\n", n, p.Type, d)
		}
		tw.Flush()
	case "show":
		if len(args) != 2 {
			return errors.New("usage: img provider show <name>")
		}
		p, ok := cfg.Providers[args[1]]
		if !ok {
			return fmt.Errorf("provider %q not found", args[1])
		}
		b, _ := toml.Marshal(config.Redact(config.Config{Version: 1, Providers: map[string]config.ProviderConfig{args[1]: p}}))
		fmt.Fprint(c.Out, string(b))
	case "use":
		if len(args) != 2 {
			return errors.New("usage: img provider use <name>")
		}
		if _, ok := cfg.Providers[args[1]]; !ok {
			return fmt.Errorf("provider %q not found", args[1])
		}
		global, err := loadGlobal(c.GlobalPath)
		if err != nil {
			return err
		}
		global.DefaultProvider = args[1]
		if err = config.Save(c.GlobalPath, global); err != nil {
			return err
		}
		fmt.Fprintf(c.Out, "Default provider: %s\n", args[1])
	case "remove":
		if len(args) != 2 {
			return errors.New("usage: img provider remove <name>")
		}
		global, err := loadGlobal(c.GlobalPath)
		if err != nil {
			return err
		}
		delete(global.Providers, args[1])
		if global.DefaultProvider == args[1] {
			global.DefaultProvider = ""
		}
		return config.Save(c.GlobalPath, global)
	case "test":
		if len(args) != 2 {
			return errors.New("usage: img provider test <name>")
		}
		pc, ok := cfg.Providers[args[1]]
		if !ok {
			return fmt.Errorf("provider %q not found", args[1])
		}
		p, err := provider.New(ctx, args[1], pc)
		if err != nil {
			return err
		}
		if tester, ok := p.(model.Tester); ok {
			if err = tester.Test(ctx); err != nil {
				return err
			}
		} else if err = p.Validate(ctx); err != nil {
			return err
		}
		fmt.Fprintln(c.Out, "available")
	default:
		return fmt.Errorf("unknown provider subcommand %q", args[0])
	}
	return nil
}
func (c *CLI) config(args []string) error {
	if len(args) == 0 {
		return errors.New("config subcommand required: path, list, validate, get, set, unset")
	}
	switch args[0] {
	case "path":
		fmt.Fprintln(c.Out, c.GlobalPath)
		return nil
	case "list":
		cfg, e := c.load()
		if e != nil {
			return e
		}
		b, _ := toml.Marshal(config.Redact(cfg))
		fmt.Fprint(c.Out, string(b))
		return nil
	case "validate":
		_, e := c.load()
		if e == nil {
			fmt.Fprintln(c.Out, "configuration is valid")
		}
		return e
	case "get":
		if len(args) != 2 {
			return errors.New("usage: img config get <key>")
		}
		cfg, e := c.load()
		if e != nil {
			return e
		}
		return get(c.Out, cfg, args[1])
	case "set":
		if len(args) != 3 {
			return errors.New("usage: img config set <key> <value>")
		}
		cfg, e := loadGlobal(c.GlobalPath)
		if e != nil {
			return e
		}
		if e = set(&cfg, args[1], args[2]); e != nil {
			return e
		}
		return config.Save(c.GlobalPath, cfg)
	case "unset":
		if len(args) != 2 {
			return errors.New("usage: img config unset <key>")
		}
		cfg, e := loadGlobal(c.GlobalPath)
		if e != nil {
			return e
		}
		if e = set(&cfg, args[1], ""); e != nil {
			return e
		}
		return config.Save(c.GlobalPath, cfg)
	default:
		return fmt.Errorf("unknown config subcommand %q", args[0])
	}
}
func loadGlobal(p string) (config.Config, error) {
	c := config.Defaults()
	b, e := os.ReadFile(p)
	if errors.Is(e, os.ErrNotExist) {
		return c, nil
	}
	if e != nil {
		return c, e
	}
	if e = toml.Unmarshal(b, &c); e != nil {
		return c, e
	}
	return c, nil
}
func pcValidate(n string, p config.ProviderConfig) error {
	switch p.Type {
	case "http":
		if p.URL == "" {
			return errors.New("--url is required for HTTP")
		}
	case "s3":
		if p.Bucket == "" || p.PublicURL == "" {
			return errors.New("--bucket and --public-url are required for S3")
		}
	case "github":
		if p.Owner == "" || p.Repo == "" || p.Token == "" {
			return errors.New("--owner, --repo, and --token are required for GitHub")
		}
	}
	_ = n
	return nil
}
func reorder(a []string, withValue map[string]bool) ([]string, error) {
	var flags, pos []string
	for i := 0; i < len(a); i++ {
		x := a[i]
		key := strings.SplitN(x, "=", 2)[0]
		if strings.HasPrefix(x, "--") {
			flags = append(flags, x)
			if withValue[key] && !strings.Contains(x, "=") {
				if i+1 >= len(a) {
					return nil, fmt.Errorf("flag %s requires a value", x)
				}
				i++
				flags = append(flags, a[i])
			}
		} else {
			pos = append(pos, x)
		}
	}
	return append(flags, pos...), nil
}
func get(w io.Writer, c config.Config, k string) error {
	switch k {
	case "output.format":
		fmt.Fprintln(w, c.Output.Format)
	case "output.copy":
		fmt.Fprintln(w, c.Output.Copy)
	case "output.quiet":
		fmt.Fprintln(w, c.Output.Quiet)
	case "default_provider":
		fmt.Fprintln(w, c.DefaultProvider)
	case "upload.concurrency":
		fmt.Fprintln(w, c.Upload.Concurrency)
	default:
		return fmt.Errorf("unsupported config key %q", k)
	}
	return nil
}
func set(c *config.Config, k, v string) error {
	switch k {
	case "output.format":
		if v != "" && !validFormat(v) {
			return fmt.Errorf("invalid format %q", v)
		}
		c.Output.Format = v
	case "output.copy":
		b, e := strconv.ParseBool(v)
		if v == "" {
			b = false
			e = nil
		}
		if e != nil {
			return e
		}
		c.Output.Copy = b
	case "output.quiet":
		b, e := strconv.ParseBool(v)
		if v == "" {
			b = false
			e = nil
		}
		if e != nil {
			return e
		}
		c.Output.Quiet = b
	case "default_provider":
		c.DefaultProvider = v
	case "upload.concurrency":
		n, e := strconv.Atoi(v)
		if e != nil || n < 1 {
			return errors.New("concurrency must be a positive integer")
		}
		c.Upload.Concurrency = n
	default:
		return fmt.Errorf("unsupported config key %q", k)
	}
	return nil
}
func validFormat(s string) bool       { return s == "url" || s == "markdown" || s == "json" || s == "html" }
func mustwd() string                  { d, _ := os.Getwd(); return d }
func readline(r *bufio.Reader) string { s, _ := r.ReadString('\n'); return s }
func (c *CLI) usage() {
	fmt.Fprintln(c.Out,
		"Usage: img upload <file...> [options]\n"+
			"       img <file...> [options]\n"+
			"       img screenshot [--region|--window] [options]\n"+
			"       img serve [--port 36677] [options]\n"+
			"       img rewrite [<file.md>] [options]\n"+
			"       img init | provider | config | version")
}

// formatBytes formats a byte count as a human-readable string (KB / MB).
func formatBytes(n int64) string {
	switch {
	case n >= 1<<20:
		return fmt.Sprintf("%.1f MB", float64(n)/(1<<20))
	case n >= 1<<10:
		return fmt.Sprintf("%.0f KB", float64(n)/(1<<10))
	default:
		return fmt.Sprintf("%d B", n)
	}
}

var _ = json.Marshal
