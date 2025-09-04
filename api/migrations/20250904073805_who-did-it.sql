-- Add created_by to releases table  
ALTER TABLE release
ADD COLUMN created_by INTEGER REFERENCES auth.users(id);

-- Add created_by to deployments table
ALTER TABLE deployment
ADD COLUMN created_by INTEGER REFERENCES auth.users(id);

