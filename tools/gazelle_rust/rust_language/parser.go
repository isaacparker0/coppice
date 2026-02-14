package rust_language

import (
	"encoding/binary"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"

	"github.com/bazelbuild/rules_go/go/runfiles"
	"google.golang.org/protobuf/proto"

	messages "coppice/tools/gazelle_rust/proto"
)

// Parser manages IPC with the Rust parser binary.
type Parser struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout io.ReadCloser
}

// Start the Rust parser subprocess.
func NewParser() *Parser {
	r, err := runfiles.New()
	if err != nil {
		log.Fatal(err)
	}

	path, err := r.Rlocation("coppice/tools/gazelle_rust/rust_parser/main")
	if err != nil {
		log.Fatal(err)
	}

	cmd := exec.Command(path, "serve")
	stdin, err := cmd.StdinPipe()
	if err != nil {
		log.Fatal(err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		log.Fatal(err)
	}
	cmd.Stderr = os.Stderr

	if err := cmd.Start(); err != nil {
		log.Fatal(err)
	}

	return &Parser{
		cmd:    cmd,
		stdin:  stdin,
		stdout: stdout,
	}
}

// Terminate the parser subprocess.
func (p *Parser) Close() error {
	p.stdin.Close()
	return p.cmd.Wait()
}

func (p *Parser) Parse(filePath string) (*messages.ParseResponse, error) {
	request := &messages.ParseRequest{
		FilePath: filePath,
	}

	data, err := proto.Marshal(request)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	// Length-prefixed protobuf protocol (little-endian u32 size + message
	// bytes).
	sizeBytes := make([]byte, 4)
	binary.LittleEndian.PutUint32(sizeBytes, uint32(len(data)))
	if _, err := p.stdin.Write(sizeBytes); err != nil {
		return nil, fmt.Errorf("write size: %w", err)
	}
	if _, err := p.stdin.Write(data); err != nil {
		return nil, fmt.Errorf("write message: %w", err)
	}

	if _, err := io.ReadFull(p.stdout, sizeBytes); err != nil {
		return nil, fmt.Errorf("read response size: %w", err)
	}
	responseSize := binary.LittleEndian.Uint32(sizeBytes)

	responseData := make([]byte, responseSize)
	if _, err := io.ReadFull(p.stdout, responseData); err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}

	response := &messages.ParseResponse{}
	if err := proto.Unmarshal(responseData, response); err != nil {
		return nil, fmt.Errorf("unmarshal response: %w", err)
	}

	if !response.Success {
		return nil, fmt.Errorf("parse error: %s", response.ErrorMsg)
	}

	return response, nil
}
