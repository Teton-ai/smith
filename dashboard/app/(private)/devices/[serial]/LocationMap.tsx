"use client";

import L from "leaflet";
import type React from "react";
import { useEffect, useState } from "react";
import { MapContainer, Marker, Popup, TileLayer } from "react-leaflet";
import "leaflet/dist/leaflet.css";

const customIcon = new L.Icon({
	iconUrl:
		"https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.7.1/images/marker-icon.png",
	iconRetinaUrl:
		"https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.7.1/images/marker-icon-2x.png",
	shadowUrl:
		"https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.7.1/images/marker-shadow.png",
	iconSize: [25, 41],
	iconAnchor: [12, 41],
	popupAnchor: [1, -34],
	shadowSize: [41, 41],
});

interface LocationMapProps {
	countryCode?: string;
	city?: string;
	country?: string;
}

const LocationMap: React.FC<LocationMapProps> = ({
	countryCode,
	city,
	country,
}) => {
	const [coordinates, setCoordinates] = useState<[number, number] | null>(null);
	const [loading, setLoading] = useState(true);

	const locationText =
		[city, country].filter(Boolean).join(", ") || "Unknown Location";

	useEffect(() => {
		const geocodeLocation = async () => {
			try {
				let query = "";
				if (city && countryCode) {
					query = `${city}, ${countryCode}`;
				} else if (city && country) {
					query = `${city}, ${country}`;
				} else if (country) {
					query = country;
				} else if (countryCode) {
					query = countryCode;
				}

				if (!query) {
					setLoading(false);
					return;
				}

				const response = await fetch(
					`https://nominatim.openstreetmap.org/search?format=json&q=${encodeURIComponent(query)}&limit=1`,
				);
				const data = await response.json();

				if (data && data.length > 0) {
					const lat = parseFloat(data[0].lat);
					const lng = parseFloat(data[0].lon);
					setCoordinates([lat, lng]);
				}
			} catch (error) {
				console.error("Geocoding error:", error);
			} finally {
				setLoading(false);
			}
		};

		geocodeLocation();
	}, [city, countryCode, country]);

	if (loading) {
		return (
			<div className="h-64 w-full rounded-lg overflow-hidden border border-gray-200 bg-gray-100 flex items-center justify-center">
				<div className="text-center">
					<div className="animate-spin rounded-full h-6 w-6 border-b-2 border-blue-600 mx-auto mb-2"></div>
					<p className="text-gray-600 text-sm">Loading map...</p>
				</div>
			</div>
		);
	}

	if (!coordinates) {
		return (
			<div className="h-64 w-full rounded-lg overflow-hidden border-2 border-dashed border-gray-300 bg-gray-50 flex items-center justify-center">
				<div className="text-center">
					<div className="text-gray-400 mb-2">🗺️</div>
					<p className="text-gray-500 text-sm">Unable to locate on map</p>
				</div>
			</div>
		);
	}

	const [lat, lng] = coordinates;

	return (
		<div className="h-64 w-full rounded-lg overflow-hidden border border-gray-200">
			<MapContainer
				center={[lat, lng]}
				zoom={10}
				style={{ height: "100%", width: "100%" }}
				scrollWheelZoom={false}
			>
				<TileLayer
					attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
					url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
				/>
				<Marker position={[lat, lng]} icon={customIcon}>
					<Popup>
						<div className="text-center">
							<div className="font-medium text-gray-900 mb-1">
								{locationText}
							</div>
							<div className="text-xs text-gray-600 font-mono">
								{lat.toFixed(4)}, {lng.toFixed(4)}
							</div>
						</div>
					</Popup>
				</Marker>
			</MapContainer>
		</div>
	);
};

export default LocationMap;
