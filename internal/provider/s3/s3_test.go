package s3provider

import (
	"context"
	"io"
	"os"
	"strings"
	"testing"

	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/smithy-go"
	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
)

type fakeS3 struct {
	headErr error
	put     *s3.PutObjectInput
	heads   int
}

func s3request(t *testing.T, file, remote string) model.UploadRequest {
	f, err := os.Open(file)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { f.Close() })
	st, _ := f.Stat()
	return model.UploadRequest{LocalPath: file, FileName: "x.png", Body: f, Size: st.Size(), RemotePath: remote, ContentType: "image/png"}
}

func (f *fakeS3) HeadObject(context.Context, *s3.HeadObjectInput, ...func(*s3.Options)) (*s3.HeadObjectOutput, error) {
	f.heads++
	return nil, f.headErr
}
func (f *fakeS3) HeadBucket(context.Context, *s3.HeadBucketInput, ...func(*s3.Options)) (*s3.HeadBucketOutput, error) {
	return &s3.HeadBucketOutput{}, nil
}
func (f *fakeS3) PutObject(_ context.Context, in *s3.PutObjectInput, _ ...func(*s3.Options)) (*s3.PutObjectOutput, error) {
	f.put = in
	io.Copy(io.Discard, in.Body)
	return &s3.PutObjectOutput{}, nil
}
func TestUploadAndEscapedURL(t *testing.T) {
	file := t.TempDir() + "/x.png"
	os.WriteFile(file, []byte("png"), 0600)
	api := &fakeS3{headErr: &smithy.GenericAPIError{Code: "NotFound", Message: "no"}}
	p := NewWithClient("r2", config.ProviderConfig{Bucket: "images", PublicURL: "https://img.test"}, api)
	r, e := p.Upload(context.Background(), s3request(t, file, "中文/a b.png"))
	if e != nil {
		t.Fatal(e)
	}
	if r.URL != "https://img.test/%E4%B8%AD%E6%96%87/a%20b.png" {
		t.Fatal(r.URL)
	}
	if *api.put.ContentType != "image/png" {
		t.Fatal("content type")
	}
}
func TestHeadErrorsAndOverwrite(t *testing.T) {
	file := t.TempDir() + "/x"
	os.WriteFile(file, []byte("x"), 0600)
	api := &fakeS3{headErr: &smithy.GenericAPIError{Code: "AccessDenied", Message: "no"}}
	p := NewWithClient("s", config.ProviderConfig{Bucket: "b", PublicURL: "https://x"}, api)
	_, e := p.Upload(context.Background(), s3request(t, file, "x"))
	if e == nil || !strings.Contains(e.Error(), "AccessDenied") {
		t.Fatalf("expected retained error: %v", e)
	}
	req := s3request(t, file, "x")
	req.Overwrite = true
	_, e = p.Upload(context.Background(), req)
	if e != nil {
		t.Fatal(e)
	}
	if api.heads != 1 {
		t.Fatalf("overwrite made head call")
	}
}
