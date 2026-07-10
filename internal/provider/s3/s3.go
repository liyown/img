package s3provider

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"os"
	"strings"

	"github.com/aws/aws-sdk-go-v2/aws"
	awsconfig "github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/smithy-go"
	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	"github.com/liyown/img/internal/pathgen"
)

type API interface {
	PutObject(context.Context, *s3.PutObjectInput, ...func(*s3.Options)) (*s3.PutObjectOutput, error)
	HeadObject(context.Context, *s3.HeadObjectInput, ...func(*s3.Options)) (*s3.HeadObjectOutput, error)
}
type Provider struct {
	name   string
	cfg    config.ProviderConfig
	client API
}

func New(ctx context.Context, name string, c config.ProviderConfig) (*Provider, error) {
	if c.Region == "" {
		c.Region = "auto"
	}
	// "when required" avoids the aws-chunked checksum encoding that several
	// S3-compatible services (notably Alibaba Cloud OSS) do not accept.
	opts := []func(*awsconfig.LoadOptions) error{
		awsconfig.WithRegion(c.Region),
		awsconfig.WithRequestChecksumCalculation(aws.RequestChecksumCalculationWhenRequired),
	}
	if c.AccessKey != "" || c.SecretKey != "" {
		opts = append(opts, awsconfig.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(c.AccessKey, c.SecretKey, c.SessionToken)))
	}
	awscfg, err := awsconfig.LoadDefaultConfig(ctx, opts...)
	if err != nil {
		return nil, fmt.Errorf("load AWS configuration: %w", err)
	}
	client := s3.NewFromConfig(awscfg, func(o *s3.Options) {
		o.UsePathStyle = c.PathStyle
		if c.Endpoint != "" {
			o.BaseEndpoint = aws.String(c.Endpoint)
		}
	})
	return &Provider{name: name, cfg: c, client: client}, nil
}
func NewWithClient(name string, c config.ProviderConfig, client API) *Provider {
	return &Provider{name: name, cfg: c, client: client}
}
func (p *Provider) Name() string { return p.name }
func (p *Provider) Type() string { return "s3" }
func (p *Provider) Validate(context.Context) error {
	if p.cfg.Bucket == "" {
		return fmt.Errorf("s3 provider %q: bucket is required", p.name)
	}
	if p.cfg.PublicURL == "" {
		return fmt.Errorf("s3 provider %q: public_url is required to produce an accessible URL", p.name)
	}
	if _, e := url.ParseRequestURI(p.cfg.PublicURL); e != nil {
		return fmt.Errorf("s3 provider %q: invalid public_url: %w", p.name, e)
	}
	return nil
}
func (p *Provider) Upload(ctx context.Context, r model.UploadRequest) (*model.UploadResult, error) {
	if err := p.Validate(ctx); err != nil {
		return nil, err
	}
	if !r.Overwrite {
		_, err := p.client.HeadObject(ctx, &s3.HeadObjectInput{Bucket: aws.String(p.cfg.Bucket), Key: aws.String(r.RemotePath)})
		if err == nil {
			return nil, fmt.Errorf("remote object %q already exists; use --overwrite", r.RemotePath)
		}
		var apiErr smithy.APIError
		if !errors.As(err, &apiErr) || (apiErr.ErrorCode() != "NotFound" && apiErr.ErrorCode() != "NoSuchKey" && apiErr.ErrorCode() != "404") {
			return nil, fmt.Errorf("check S3 object %q: %w", r.RemotePath, err)
		}
	}
	f, err := os.Open(r.LocalPath)
	if err != nil {
		return nil, fmt.Errorf("open upload file: %w", err)
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return nil, fmt.Errorf("stat upload file: %w", err)
	}
	_, err = p.client.PutObject(ctx, &s3.PutObjectInput{Bucket: aws.String(p.cfg.Bucket), Key: aws.String(r.RemotePath), Body: f, ContentType: aws.String(r.ContentType), ContentLength: aws.Int64(st.Size()), Metadata: r.Metadata})
	if err != nil {
		return nil, fmt.Errorf("upload %q to S3 provider %q: %w", r.RemotePath, p.name, err)
	}
	u := strings.TrimRight(p.cfg.PublicURL, "/") + "/" + pathgen.EscapeURLPath(r.RemotePath)
	return &model.UploadResult{Provider: p.name, LocalPath: r.LocalPath, RemotePath: r.RemotePath, URL: u, Size: st.Size(), ContentType: r.ContentType}, nil
}
