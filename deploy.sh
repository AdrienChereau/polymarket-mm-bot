#!/usr/bin/env bash
# deploy.sh — déploiement automatisé de l'architecture monolithe distribuée.
# À exécuter sur chaque instance EC2 (radar : ap-northeast-1 / executor : eu-west-1).
set -euo pipefail

echo "=== [1/5] Récupération des dernières sources ==="
git pull origin main

echo "=== [2/5] Compilation du Backend Rust (Release) ==="
cd backend
# Cible explicite pour la cross-compilation / cohérence des instances.
cargo build --release --target x86_64-unknown-linux-gnu || cargo build --release
cd ..

echo "=== [3/5] Déploiement du Dashboard statique vers Nginx ==="
sudo rm -rf /var/www/html/*
sudo cp -r frontend/* /var/www/html/
echo "Frontend copié dans /var/www/html"

echo "=== [4/5] Installation du service Systemd ==="
sudo mkdir -p /opt/polymarket-monolith/backend/data
sudo cp polymarket-mm.service /etc/systemd/system/
sudo systemctl daemon-reload

echo "=== [5/5] Redémarrage du moteur ==="
sudo systemctl restart polymarket-mm.service
sudo systemctl enable polymarket-mm.service

echo "=== DÉPLOIEMENT TERMINÉ ==="
sudo systemctl status polymarket-mm.service --no-pager | head -n 12
