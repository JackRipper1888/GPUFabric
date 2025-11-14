## execute GUI python script
### step 1: create virtual environment
```bash
python3 -m venv . venv
```
### step 2: activate virtual environment
```bash
source .venv/bin/activate
```
### step 3: install dependencies
```bash
pip install -r requirements.txt
# if there is no requirements.txt
pip install matplotlib pyqt5 pandas  psycopg2-binary 
pip freeze > requirements.txt
```
### step 4: run script
```bash
python3 stats_dashboard.py
```
### step 5: exit virtual environment
```bash
deactivate
```