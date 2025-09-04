-- Add created_by to releases table  
ALTER TABLE release
ADD COLUMN user_id INTEGER REFERENCES auth.users(id) ON DELETE SET NULL;

