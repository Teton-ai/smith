export type SystemInfo = {
    connection_statuses?: Array<{
      connection_name: string;
      connection_state: string;
      device_name: string;
      device_type: string;
    }>;
    network?: {
      interfaces?: Record<string, {
        ips: string[];
        mac_address: string;
      }>;
    };
  device_tree?: {
    model?: string
    serial_number?: string
      compatible?: string[];
  }
  hostname?: string
  os_release?: {
    pretty_name?: string
    version_id?: string
  }
  smith?: {
    version?: string
  }
  proc?: {
    version?: string;
      stat?: {
        btime?: number;
      };
  }
}

export type DeviceNetwork = {
  network_score?: number
  download_speed_mbps?: number
  upload_speed_mbps?: number
  source?: string
  updated_at?: string
}

export type IpAddressInfo = {
  id: number;
  ip_address: string;
  name?: string;
  continent?: string;
  continent_code?: string;
  country_code?: string;
  country?: string;
  region?: string;
  city?: string;
  isp?: string;
  coordinates?: [number, number];
  proxy?: boolean;
  hosting?: boolean;
  device_count?: number;
  created_at: string;
  updated_at: string;
}

export type Device = {
  id: number;
  serial_number: string;
  note?: string
  hostname?: string;
  last_seen?: string;
  created_on: string;
  has_token: boolean;
  release_id?: number;
  release?: Release;
  target_release_id?: number;
  target_release?: Release;
  approved: boolean
  network?: DeviceNetwork;
  ip_address_id?: number;
  ip_address?: IpAddressInfo;
  modem_id?: number;
  modem?: Modem;
  system_info?: SystemInfo;
  labels: Record<string, string>
}


export type Release = {
  id: number;
  distribution_id: number;
  distribution_architecture: string;
  distribution_name: string;
  version: string;
  draft: boolean;
  yanked: boolean;
  created_at: string;
  size?: number;
  download_count?: number;
  user_id?: number;
  user_email?: string;
}

export type Deployment = {
  id: number;
  release_id: number;
  status: 'InProgress' | 'Done' | 'Failed' | 'Canceled';
  updated_at: string;
  created_at: string;
}

export type Modem = {
  id: number
  imei: string
  network_provider: string
  updated_at: string
  created_at: string
  on_dongle?: boolean;
}

export type Distribution = {
  id: number;
  name: string;
  description?: string;
  architecture: string;
  num_packages?: number;
}

export type Rollout = {
  distribution_id: number;
  pending_devices?: number;
  total_devices?: number;
  updated_devices?: number;
}
