-- Migration 001: Create reimbursement tables
-- This extends the base a2a-rs task tables with reimbursement-specific data

-- Create reimbursement_requests table
CREATE TABLE IF NOT EXISTS reimbursement_requests (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    request_type TEXT NOT NULL, -- 'initial', 'form_submission', 'status_query'
    
    -- Request data
    date TEXT,
    amount_value REAL,
    amount_currency TEXT,
    purpose TEXT,
    category TEXT,
    notes TEXT,
    
    -- Status tracking
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'under_review', 'approved', 'rejected', 'requires_additional_info'
    
    -- Approval data
    approved_amount_value REAL,
    approved_amount_currency TEXT,
    approval_date TEXT,
    approver TEXT,
    rejection_reason TEXT,
    
    -- Metadata
    metadata TEXT, -- JSON string
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Create receipts table
CREATE TABLE IF NOT EXISTS receipts (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    file_name TEXT NOT NULL,
    mime_type TEXT,
    size_bytes INTEGER,
    upload_timestamp TEXT,
    
    -- Extracted data (from OCR/processing)
    extracted_vendor TEXT,
    extracted_date TEXT,
    extracted_amount_value REAL,
    extracted_amount_currency TEXT,
    confidence_score REAL,
    extracted_data TEXT, -- JSON string for additional extracted data
    
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (request_id) REFERENCES reimbursement_requests(id) ON DELETE CASCADE
);

-- Create approval_workflow table for tracking approval steps
CREATE TABLE IF NOT EXISTS approval_workflow (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    step_number INTEGER NOT NULL,
    step_type TEXT NOT NULL, -- 'auto_approval', 'manager_approval', 'finance_approval', etc.
    status TEXT NOT NULL, -- 'pending', 'approved', 'rejected', 'skipped'
    approver TEXT,
    comments TEXT,
    decision_date TEXT,
    
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (request_id) REFERENCES reimbursement_requests(id) ON DELETE CASCADE
);

-- Create indexes for common queries
CREATE INDEX IF NOT EXISTS idx_reimbursement_requests_task_id ON reimbursement_requests(task_id);
CREATE INDEX IF NOT EXISTS idx_reimbursement_requests_status ON reimbursement_requests(status);
CREATE INDEX IF NOT EXISTS idx_reimbursement_requests_created_at ON reimbursement_requests(created_at);
CREATE INDEX IF NOT EXISTS idx_receipts_request_id ON receipts(request_id);
CREATE INDEX IF NOT EXISTS idx_approval_workflow_request_id ON approval_workflow(request_id);
CREATE INDEX IF NOT EXISTS idx_approval_workflow_status ON approval_workflow(status);

-- Create triggers for updated_at
CREATE TRIGGER IF NOT EXISTS update_reimbursement_requests_updated_at
AFTER UPDATE ON reimbursement_requests
FOR EACH ROW
BEGIN
    UPDATE reimbursement_requests SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS update_approval_workflow_updated_at
AFTER UPDATE ON approval_workflow
FOR EACH ROW
BEGIN
    UPDATE approval_workflow SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;