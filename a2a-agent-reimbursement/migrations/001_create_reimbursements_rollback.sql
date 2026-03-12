-- Rollback migration 001: Drop reimbursement tables

-- Drop triggers first
DROP TRIGGER IF EXISTS update_reimbursement_requests_updated_at;
DROP TRIGGER IF EXISTS update_approval_workflow_updated_at;

-- Drop indexes
DROP INDEX IF EXISTS idx_reimbursement_requests_task_id;
DROP INDEX IF EXISTS idx_reimbursement_requests_status;
DROP INDEX IF EXISTS idx_reimbursement_requests_created_at;
DROP INDEX IF EXISTS idx_receipts_request_id;
DROP INDEX IF EXISTS idx_approval_workflow_request_id;
DROP INDEX IF EXISTS idx_approval_workflow_status;

-- Drop tables in reverse order of dependencies
DROP TABLE IF EXISTS approval_workflow;
DROP TABLE IF EXISTS receipts;
DROP TABLE IF EXISTS reimbursement_requests;