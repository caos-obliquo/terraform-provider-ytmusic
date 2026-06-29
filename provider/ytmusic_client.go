package provider

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os/exec"
)

type YTMusicClient struct {
	config *ProviderConfig
}

func NewYTMusicClient(config *ProviderConfig) *YTMusicClient {
	return &YTMusicClient{config: config}
}

// Request/Response protocol matching ytmusic-cli

type ClientRequest struct {
	Action     string           `json:"action"`
	CookieFile *string          `json:"cookie_file,omitempty"`
	Payload    *json.RawMessage `json:"payload,omitempty"`
}

type ClientResponse struct {
	Success bool             `json:"success"`
	Data    *json.RawMessage `json:"data,omitempty"`
	Error   *string          `json:"error,omitempty"`
}

func (c *YTMusicClient) call(action string, payload interface{}) (*ClientResponse, error) {
	var payloadJSON json.RawMessage
	if payload != nil {
		b, err := json.Marshal(payload)
		if err != nil {
			return nil, fmt.Errorf("marshal payload: %w", err)
		}
		payloadJSON = b
	}

	req := ClientRequest{
		Action:  action,
		Payload: &payloadJSON,
	}
	if c.config.CookieFile != "" {
		req.CookieFile = &c.config.CookieFile
	}

	reqBody, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	cmd := exec.Command(c.config.CLIPath)
	cmd.Stdin = bytes.NewReader(reqBody)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("exec %s: %w\nstderr: %s", c.config.CLIPath, err, stderr.String())
	}

	var resp ClientResponse
	if err := json.Unmarshal(stdout.Bytes(), &resp); err != nil {
		return nil, fmt.Errorf("parse response: %w\nraw: %s", err, stdout.String())
	}

	if !resp.Success && resp.Error != nil {
		return &resp, fmt.Errorf("ytmusic error: %s", *resp.Error)
	}

	return &resp, nil
}

func (c *YTMusicClient) AuthCheck() error {
	_, err := c.call("auth-check", nil)
	return err
}

// PlaylistCRUD

type PlaylistCreateInput struct {
	Title       string  `json:"title"`
	Description *string `json:"description,omitempty"`
	Privacy     *string `json:"privacy,omitempty"`
}

type PlaylistCreateOutput struct {
	ID string `json:"id"`
}

func (c *YTMusicClient) CreatePlaylist(input PlaylistCreateInput) (*PlaylistCreateOutput, error) {
	resp, err := c.call("playlist-create", input)
	if err != nil {
		return nil, err
	}
	var out PlaylistCreateOutput
	if err := json.Unmarshal(*resp.Data, &out); err != nil {
		return nil, fmt.Errorf("parse create output: %w", err)
	}
	return &out, nil
}

type PlaylistDeleteInput struct {
	ID string `json:"id"`
}

func (c *YTMusicClient) DeletePlaylist(id string) error {
	_, err := c.call("playlist-delete", PlaylistDeleteInput{ID: id})
	return err
}

type PlaylistEditInput struct {
	ID          string  `json:"id"`
	Title       *string `json:"title,omitempty"`
	Description *string `json:"description,omitempty"`
	Privacy     *string `json:"privacy,omitempty"`
}

func (c *YTMusicClient) EditPlaylist(id string, title, description, privacy *string) error {
	_, err := c.call("playlist-edit", PlaylistEditInput{
		ID: id, Title: title, Description: description, Privacy: privacy,
	})
	return err
}

type PlaylistSummary struct {
	ID          string  `json:"id"`
	Title       string  `json:"title"`
	Description *string `json:"description,omitempty"`
	TrackCount  *uint32 `json:"track_count,omitempty"`
}

func (c *YTMusicClient) ListPlaylists() ([]PlaylistSummary, error) {
	resp, err := c.call("playlist-list", nil)
	if err != nil {
		return nil, err
	}
	var list []PlaylistSummary
	if err := json.Unmarshal(*resp.Data, &list); err != nil {
		return nil, fmt.Errorf("parse list output: %w", err)
	}
	return list, nil
}

type PlaylistGetInput struct {
	ID string `json:"id"`
}

func (c *YTMusicClient) GetPlaylist(id string) (*json.RawMessage, error) {
	resp, err := c.call("playlist-get", PlaylistGetInput{ID: id})
	if err != nil {
		return nil, err
	}
	return resp.Data, nil
}

type AddItemsInput struct {
	ID       string   `json:"id"`
	VideoIDs []string `json:"video_ids"`
}

func (c *YTMusicClient) AddItems(id string, videoIDs []string) error {
	_, err := c.call("playlist-add-items", AddItemsInput{ID: id, VideoIDs: videoIDs})
	return err
}

type RemoveItemsInput struct {
	ID          string   `json:"id"`
	SetVideoIDs []string `json:"set_video_ids"`
}

func (c *YTMusicClient) RemoveItems(id string, setVideoIDs []string) error {
	_, err := c.call("playlist-remove-items", RemoveItemsInput{ID: id, SetVideoIDs: setVideoIDs})
	return err
}

// Search

type SearchInput struct {
	Query      string  `json:"query"`
	SearchType *string `json:"type,omitempty"`
	Limit      *uint32 `json:"limit,omitempty"`
}

func (c *YTMusicClient) Search(query, searchType string) (*json.RawMessage, error) {
	t := searchType
	if t == "" {
		t = "songs"
	}
	resp, err := c.call("search", SearchInput{Query: query, SearchType: &t})
	if err != nil {
		return nil, err
	}
	return resp.Data, nil
}
