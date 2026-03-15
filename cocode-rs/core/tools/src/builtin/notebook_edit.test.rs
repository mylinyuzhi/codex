use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

fn create_test_notebook() -> String {
    serde_json::json!({
        "cells": [
            {
                "cell_type": "markdown",
                "id": "cell-1",
                "metadata": {},
                "source": ["# Test Notebook\n"]
            },
            {
                "cell_type": "code",
                "id": "cell-2",
                "metadata": {},
                "source": ["print('hello')\n"],
                "outputs": [],
                "execution_count": null
            }
        ],
        "metadata": {
            "kernelspec": {
                "display_name": "Python 3",
                "language": "python",
                "name": "python3"
            }
        },
        "nbformat": 4,
        "nbformat_minor": 5
    })
    .to_string()
}

#[tokio::test]
async fn test_replace_cell() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_id": "cell-2",
        "new_source": "print('modified')",
        "edit_mode": "replace"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify the content was changed
    let content = std::fs::read_to_string(&notebook_path).unwrap();
    assert!(content.contains("modified"));
    assert!(!content.contains("hello"));
}

#[tokio::test]
async fn test_insert_cell() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_id": "cell-1",
        "cell_type": "code",
        "new_source": "# New cell",
        "edit_mode": "insert"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify the cell was inserted
    let content = std::fs::read_to_string(&notebook_path).unwrap();
    let notebook: Notebook = serde_json::from_str(&content).unwrap();
    assert_eq!(notebook.cells.len(), 3);
}

#[tokio::test]
async fn test_delete_cell() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_id": "cell-2",
        "new_source": "",
        "edit_mode": "delete"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify the cell was deleted
    let content = std::fs::read_to_string(&notebook_path).unwrap();
    let notebook: Notebook = serde_json::from_str(&content).unwrap();
    assert_eq!(notebook.cells.len(), 1);
}

#[tokio::test]
async fn test_requires_read_first() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    // Don't read the file first

    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_id": "cell-2",
        "new_source": "print('modified')",
        "edit_mode": "replace"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_rejects_non_ipynb() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.py");
    std::fs::write(&file_path, "print('hello')").unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "notebook_path": file_path.to_str().unwrap(),
        "cell_id": "cell-1",
        "new_source": "print('modified')"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains(".ipynb"));
}

#[test]
fn test_tool_properties() {
    let tool = NotebookEditTool::new();
    assert_eq!(tool.name(), "NotebookEdit");
    assert!(!tool.is_concurrent_safe());
    assert!(!tool.is_read_only());
}

#[tokio::test]
async fn test_replace_cell_by_number() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    // Replace cell at index 1 (the code cell)
    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_number": 1,
        "new_source": "print('replaced by number')",
        "edit_mode": "replace"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&notebook_path).unwrap();
    assert!(content.contains("replaced by number"));
    assert!(!content.contains("hello"));
}

#[tokio::test]
async fn test_insert_cell_by_number() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    // Insert cell at position 1 (between markdown and code cells)
    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_number": 1,
        "cell_type": "code",
        "new_source": "# Inserted at position 1",
        "edit_mode": "insert"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&notebook_path).unwrap();
    let notebook: Notebook = serde_json::from_str(&content).unwrap();
    assert_eq!(notebook.cells.len(), 3);
    // The new cell should be at index 1
    assert!(
        notebook.cells[1]
            .source
            .to_string()
            .contains("Inserted at position 1")
    );
}

#[tokio::test]
async fn test_delete_cell_by_number() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    // Delete cell at index 0 (the markdown cell)
    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_number": 0,
        "new_source": "",
        "edit_mode": "delete"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&notebook_path).unwrap();
    let notebook: Notebook = serde_json::from_str(&content).unwrap();
    assert_eq!(notebook.cells.len(), 1);
    // Only the code cell should remain
    assert_eq!(notebook.cells[0].cell_type, "code");
}

#[tokio::test]
async fn test_cell_number_out_of_bounds() {
    let dir = TempDir::new().unwrap();
    let notebook_path = dir.path().join("test.ipynb");
    std::fs::write(&notebook_path, create_test_notebook()).unwrap();

    let tool = NotebookEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&notebook_path).await;

    // Try to replace cell at index 99 (out of bounds)
    let input = serde_json::json!({
        "notebook_path": notebook_path.to_str().unwrap(),
        "cell_number": 99,
        "new_source": "should fail",
        "edit_mode": "replace"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("out of bounds"));
}
