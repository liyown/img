package model

import "context"

type UploadRequest struct {
	LocalPath   string
	RemotePath  string
	ContentType string
	Overwrite   bool
	Metadata    map[string]string
}

type UploadResult struct {
	Provider    string `json:"provider"`
	LocalPath   string `json:"local_path"`
	RemotePath  string `json:"remote_path"`
	URL         string `json:"url"`
	Size        int64  `json:"size"`
	ContentType string `json:"content_type"`
}

type Provider interface {
	Name() string
	Type() string
	Validate(context.Context) error
	Upload(context.Context, UploadRequest) (*UploadResult, error)
}
