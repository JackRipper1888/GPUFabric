import sys
import signal
import psycopg2
from psycopg2 import pool
import time
from PyQt5.QtWidgets import (QApplication, QMainWindow, QWidget, QVBoxLayout, 
                           QHBoxLayout, QLabel, QComboBox, QDateEdit, 
                           QPushButton, QTabWidget, QTableWidget, 
                           QTableWidgetItem, QHeaderView, QFileDialog,
                           QMessageBox)
from PyQt5.QtCore import QDate, Qt, QLocale
from matplotlib.backends.backend_qt5agg import FigureCanvasQTAgg as FigureCanvas
from matplotlib.figure import Figure
import pandas as pd
from datetime import datetime, timedelta
import traceback
import matplotlib as mpl
from matplotlib.font_manager import FontProperties

# Set Chinese font support
try:
    # Try to use system fonts for Chinese characters
    mpl.rcParams['font.sans-serif'] = ['Arial Unicode MS', 'SimHei', 'Microsoft YaHei', 'WenQuanYi Micro Hei', 'DejaVu Sans']
    mpl.rcParams['axes.unicode_minus'] = False
except:
    pass

class StatsDashboard(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("GPU Usage Statistics Dashboard")
        self.setGeometry(100, 100, 1200, 800)
        
        # Database connection configuration
        self.db_config = {
            'dbname': 'GPUFabric',
            'user': 'postgres',
            'password': 'password',
            'host': 'localhost',
            'port': '5432'
        }
        
        # Initialize connection pool
        try:
            self.db_pool = psycopg2.pool.SimpleConnectionPool(
                minconn=3,  # Minimum connections
                maxconn=20,  # Maximum connections
                **self.db_config
            )
            # Test if connection pool works
            conn = self.get_db_connection()
            if not conn:
                raise Exception("Cannot establish initial database connection")
            self.release_db_connection(conn)
            print("Database connection pool initialized successfully")
        except Exception as e:
            print(f"Failed to create database connection pool: {e}")
            QMessageBox.critical(self, "Error", f"Cannot connect to database: {e}")
            sys.exit(1)
        
        # Initialize the UI
        self.init_ui()
        
        # Load data after UI is initialized
        self.load_data()
    
    def get_db_connection(self, max_retries=3, retry_delay=1):
        """Get database connection with retry mechanism"""
        last_exception = None
        conn = None
        for attempt in range(max_retries):
            try:
                if not self.db_pool:
                    raise Exception("Database connection pool not initialized")
                conn = self.db_pool.getconn()
                # Test the connection before returning it
                with conn.cursor() as cur:
                    cur.execute('SELECT 1')
                return conn
            except Exception as e:
                if conn:
                    try:
                        self.db_pool.putconn(conn, close=True)
                    except:
                        pass
                    conn = None
                last_exception = e
                print(f"Failed to get database connection, retry {attempt + 1}: {e}")
                time.sleep(retry_delay)
        
        print(f"Failed to get database connection after {max_retries} retries: {last_exception}")
        QMessageBox.warning(self, "Warning", "Cannot get database connection, please try again later")
        return None

    def release_db_connection(self, conn):
        """Release database connection back to pool"""
        if not conn or not self.db_pool:
            return
        
        try:
            if not conn.closed:
                try:
                    if conn.get_transaction_status() != psycopg2.extensions.TRANSACTION_STATUS_IDLE:
                        conn.rollback()
                except:
                    pass
                self.db_pool.putconn(conn)
        except Exception as e:
            print(f"Error releasing database connection: {e}")
            # If error occurs, try to close connection
            try:
                conn.close()
            except:
                pass
            # Ensure connection is marked as unavailable
            self.db_pool.putconn(conn, close=True)

    def closeEvent(self, event):
        """Override close event to ensure all database connections are closed"""
        if hasattr(self, 'db_pool') and self.db_pool:
            self.db_pool.closeall()
        event.accept()

    def init_ui(self):
        """Initialize user interface"""
        main_widget = QWidget()
        self.setCentralWidget(main_widget)
        layout = QVBoxLayout(main_widget)
        
        # Top control panel
        control_panel = self.create_control_panel()
        layout.addLayout(control_panel)
        
        # Create tab widget
        self.tabs = QTabWidget()
        
        # Client statistics tab
        self.client_tab = QWidget()
        self.client_layout = QVBoxLayout(self.client_tab)
        self.client_figure = Figure(figsize=(10, 6), dpi=100)
        self.client_canvas = FigureCanvas(self.client_figure)
        self.client_layout.addWidget(self.client_canvas)
        
        # Device statistics tab
        self.device_tab = QWidget()
        self.device_layout = QVBoxLayout(self.device_tab)
        self.device_figure = Figure(figsize=(10, 6), dpi=100)
        self.device_canvas = FigureCanvas(self.device_figure)
        self.device_layout.addWidget(self.device_canvas)
        
        # Data table tab
        self.table_tab = QWidget()
        self.table_layout = QVBoxLayout(self.table_tab)
        self.data_table = QTableWidget()
        self.data_table.setColumnCount(0)
        self.data_table.setRowCount(0)
        self.table_layout.addWidget(self.data_table)
        
        # Add tabs
        self.tabs.addTab(self.client_tab, "Client Statistics")
        self.tabs.addTab(self.device_tab, "Device Statistics")
        self.tabs.addTab(self.table_tab, "Detailed Data")
        
        layout.addWidget(self.tabs)
    
    def create_control_panel(self):
        """Create control panel"""
        panel = QHBoxLayout()
        
        # Date selection
        panel.addWidget(QLabel("Start Date:"))
        self.start_date = QDateEdit()
        self.start_date.setDate(QDate.currentDate().addDays(-7))
        self.start_date.setCalendarPopup(True)
        panel.addWidget(self.start_date)
        
        panel.addWidget(QLabel("End Date:"))
        self.end_date = QDateEdit()
        self.end_date.setDate(QDate.currentDate())
        self.end_date.setCalendarPopup(True)
        panel.addWidget(self.end_date)
        
        # Client selection
        panel.addWidget(QLabel("Client:"))
        self.client_combo = QComboBox()
        self.client_combo.addItem("All Clients", "all")
        self.client_combo.currentIndexChanged.connect(self.on_client_changed)
        panel.addWidget(self.client_combo)
        
        # Device selection
        panel.addWidget(QLabel("Device:"))
        self.device_combo = QComboBox()
        self.device_combo.addItem("All Devices", "all")
        panel.addWidget(self.device_combo)
        
        # Refresh button
        refresh_btn = QPushButton("Refresh Data")
        refresh_btn.clicked.connect(self.load_data)
        refresh_btn.setStyleSheet("""
            QPushButton {
                background-color: #4CAF50;
                color: white;
                border: none;
                padding: 5px 15px;
                font-weight: bold;
                border-radius: 4px;
            }
            QPushButton:hover {
                background-color: #45a049;
            }
        """)
        panel.addWidget(refresh_btn)
        
        # Export button
        export_btn = QPushButton("Export Data")
        export_btn.clicked.connect(self.export_data)
        panel.addWidget(export_btn)
        
        return panel
    
    def load_data(self, from_client_changed=False):
        """Load data
        
        Args:
            from_client_changed: Whether triggered by client change event, used to prevent circular calls
        """
        # Prevent re-entry
        if hasattr(self, '_is_loading') and self._is_loading:
            return
            
        self._is_loading = True
        conn = None
        
        try:
            conn = self.get_db_connection()
            if not conn:
                QMessageBox.warning(self, "Warning", "Cannot get database connection, please try again later")
                return
                
            # Get date range
            start_date = self.start_date.date().toString("yyyy-MM-dd")
            end_date = self.end_date.date().toString("yyyy-MM-dd")
            
            print(f"Loading data for date range: {start_date} to {end_date}")
            
            # Get currently selected client and device
            selected_client = self.client_combo.currentData()
            selected_device = self.device_combo.currentData()
            
            # Only update client and device lists if not triggered by client change event
            if not from_client_changed:
                try:
                    # Block signals to prevent triggering on_client_changed
                    self.client_combo.blockSignals(True)
                    self.device_combo.blockSignals(True)
                    
                    # Save currently selected client and device
                    current_client = self.client_combo.currentData()
                    current_device = self.device_combo.currentData()
                    
                    # Reload client and device lists
                    self.load_clients(conn)
                    self.load_devices(conn, current_client)
                    
                    # Restore selected client and device
                    if current_client:
                        index = self.client_combo.findData(current_client)
                        if index >= 0:
                            self.client_combo.setCurrentIndex(index)
                    
                    if current_device:
                        index = self.device_combo.findData(current_device)
                        if index >= 0:
                            self.device_combo.setCurrentIndex(index)
                    
                except Exception as e:
                    print(f"Failed to load client/device list: {e}")
                    traceback.print_exc()
                    QMessageBox.warning(self, "Warning", "Failed to load client/device list, please check network connection")
                    return
                finally:
                    # Restore signals
                    self.client_combo.blockSignals(False)
                    self.device_combo.blockSignals(False)
            
            # Load client statistics data
            try:
                # Clear existing client charts
                self.client_figure.clear()
                self.client_canvas.draw()
                
                # Load new client statistics data
                client_df = self.load_client_stats(conn, start_date, end_date, selected_client)
                
                # If data was returned but chart not updated, force redraw
                if client_df is not None and not client_df.empty:
                    self.plot_client_stats(client_df)
                    self.client_canvas.draw()
            except Exception as e:
                print(f"Failed to load client statistics data: {e}")
                traceback.print_exc()
                QMessageBox.warning(self, "Error", f"Failed to load client statistics data: {str(e)}")
            
            # Load device statistics data
            try:
                # Clear existing device charts
                self.device_figure.clear()
                self.device_canvas.draw()
                
                # Get selected device and client
                device_index = self.device_combo.currentData()
                client_id = self.client_combo.currentData()
                
                # Load device statistics data
                device_df = self.load_device_stats(
                    conn, 
                    start_date, 
                    end_date, 
                    device_id=device_index if device_index != "all" else None,
                    client_id=client_id if client_id != "all" else None
                )
                
                # If data was returned, plot chart
                if device_df is not None and not device_df.empty:
                    self.plot_device_stats(device_df)
                    self.device_canvas.draw()
                else:
                    print("No device statistics data found")
            except Exception as e:
                print(f"Failed to load device statistics data: {e}")
                traceback.print_exc()
                QMessageBox.warning(self, "Error", f"Failed to load device statistics data: {str(e)}")
            
            # Load table data
            try:
                current_tab = self.tabs.currentIndex()
                if current_tab == 0:  # Client statistics
                    self.load_table_data(conn, start_date, end_date, 'client', selected_client)
                else:  # Device statistics
                    if selected_client and selected_client != "all":
                        device_index = self.device_combo.currentData()
                        self.load_table_data(conn, start_date, end_date, 'device', device_index)
                    else:
                        print("No specific client selected, clear device table")
                        self.update_table([], [])
            except Exception as e:
                print(f"Failed to load table data: {e}")
                traceback.print_exc()
                QMessageBox.warning(self, "Error", f"Failed to load table data: {str(e)}")
                
        except Exception as e:
            print(f"Error occurred while loading data: {e}")
            traceback.print_exc()
            QMessageBox.critical(self, "Error", f"Error occurred while loading data: {str(e)}")
        finally:
            self.release_db_connection(conn)
            self._is_loading = False
    
    def on_client_changed(self, index):
        """ when client changed """
        if index < 0:  # invalid index
            return
            
        # prevent re-entry
        if hasattr(self, '_is_loading') and self._is_loading:
            return
            
        conn = None
        try:
            # get selected client id
            client_id = self.client_combo.currentData()
            
            conn = self.get_db_connection()
            if not conn:
                return
                
            # Block signals to prevent triggering multiple updates
            self.device_combo.blockSignals(True)
            
            try:
                # Save currently selected device
                current_device = self.device_combo.currentData()
                
                # Update device list
                self.load_devices(conn, client_id)
                
                # Restore selected device (if exists)
                if current_device:
                    device_index = self.device_combo.findData(current_device)
                    if device_index >= 0:
                        self.device_combo.setCurrentIndex(device_index)
                
                # Reload data, marked as triggered by client change event
                self.load_data(from_client_changed=True)
                
            except Exception as e:
                print(f"Error updating device list: {e}")
                traceback.print_exc()
                QMessageBox.warning(self, "Error", f"Error updating device list: {str(e)}")
                
        except Exception as e:
            print(f"Error handling client change: {e}")
            traceback.print_exc()
            QMessageBox.critical(self, "Error", f"Error handling client change: {str(e)}")
            
        finally:
            # Ensure signals are restored
            if hasattr(self, 'device_combo'):
                self.device_combo.blockSignals(False)
            if conn:
                self.release_db_connection(conn)
            
            # Reset loading state
            if hasattr(self, '_is_loading'):
                self._is_loading = False
    
    def load_clients(self, conn):
        """Load client list"""
        try:
            # Start new transaction
            conn.rollback()
            cursor = conn.cursor()
            
            # Get client information from gpu_assets table
            cursor.execute("""
                SELECT DISTINCT ga.client_id, ga.client_name
                FROM gpu_assets ga
                WHERE ga.client_id IS NOT NULL
                ORDER BY ga.client_name, ga.client_id
            """)
            
            self.client_combo.clear()
            self.client_combo.addItem("All Clients", "all")
            
            for row in cursor.fetchall():
                client_id = row[0]
                client_name = row[1] if row[1] else f"Client {client_id.hex()[:8]}..."
                display_name = f"{client_name} ({client_id.hex()[:8]}...)"
                self.client_combo.addItem(display_name, client_id)
                
            # If no data from gpu_assets table, get from client_daily_stats table
            if self.client_combo.count() <= 1:  # Only "All Clients" item
                cursor.execute("""
                    SELECT DISTINCT client_id
                    FROM client_daily_stats
                    WHERE client_id IS NOT NULL
                    ORDER BY client_id
                """)
                
                for row in cursor.fetchall():
                    client_id = row[0]
                    display_name = f"Client {client_id.hex()[:8]}... ({client_id.hex()[:8]}...)"
                    self.client_combo.addItem(display_name, client_id)
        except Exception as e:
            print(f"Failed to load client list: {e}")
            print(traceback.format_exc())
    
    def load_devices(self, conn, client_id=None):
        """Load device list
        
        Args:
            conn: Database connection
            client_id: Optional client ID, if provided only load devices for that client
        """
        try:
            # Start new transaction
            conn.rollback()
            cursor = conn.cursor()
            
            # First get actual columns in the table
            cursor.execute("""
                SELECT column_name 
                FROM information_schema.columns 
                WHERE table_name = 'device_daily_stats'
                ORDER BY ordinal_position
            """)
            columns = [row[0] for row in cursor.fetchall()]
            
            # build query
            query = """
                SELECT DISTINCT d.device_index, d.device_name, d.client_id, g.client_name
                FROM device_daily_stats d
                LEFT JOIN gpu_assets g ON d.client_id = g.client_id
            """
            
            # add client filter
            params = []
            if client_id and client_id != "all":
                query += " WHERE d.client_id = %s"
                params.append(client_id)
                
            query += " ORDER BY d.device_index"
            
            cursor.execute(query, params) if params else cursor.execute(query)
            
            self.device_combo.clear()
            self.device_combo.addItem("All Device", "all")
            
            # Store currently selected device index (if any)
            current_device = self.device_combo.currentData()
            
            # Clear device dropdown
            self.device_combo.clear()
            self.device_combo.addItem("All Device", "all")
            
            # Add devices to dropdown
            for row in cursor.fetchall():
                device_index = row[0]
                device_name = row[1] if row[1] else None
                client_name = row[3] if row[3] else f"Client {row[2].hex()[:8]}..." if row[2] else "Unknown Client"
                
                if device_name:
                    display_name = f"{device_name} (device {device_index}) - {client_name}"
                else:
                    display_name = f"device {device_index} - {client_name}"
                    
                self.device_combo.addItem(display_name, device_index)
            
            # Restore previously selected device (if still exists)
            if current_device:
                index = self.device_combo.findData(current_device)
                if index >= 0:
                    self.device_combo.setCurrentIndex(index)
        except Exception as e:
            print(f"Failed to load device list: {e}")
            print(traceback.format_exc())
    
    def load_client_stats(self, conn, start_date, end_date, client_id=None):
        """Load client statistics data
        
        Args:
            conn: database connection
            start_date: start date (yyyy-MM-dd)
            end_date: end date (yyyy-MM-dd)
            client_id: optional client ID, if provided only load data for that client
            
        Returns:
            pd.DataFrame: DataFrame containing client statistics data, returns empty DataFrame if error
        """
        cursor = None
        try:
            # start new transaction
            conn.rollback()
            cursor = conn.cursor()
            
            print(f"Loading client stats from {start_date} to {end_date}, client_id: {client_id}")
            
            # Build query to get data from client_daily_stats and gpu_assets tables
            query = """
                SELECT 
                    c.date,
                    c.client_id,
                    g.client_name,
                    c.total_heartbeats,
                    c.avg_cpu_usage,
                    c.avg_memory_usage,
                    c.avg_disk_usage,
                    c.total_network_in_bytes,
                    c.total_network_out_bytes
                FROM client_daily_stats c
                LEFT JOIN gpu_assets g ON c.client_id = g.client_id
                WHERE c.date >= %s AND c.date <= %s
            """
            
            params = [start_date, end_date]
            
            # get client_id from combo box
            if client_id is None:
                client_id = self.client_combo.currentData()
                
            if client_id and client_id != "all":
                query += " AND c.client_id = %s"
                params.append(client_id)
            
            # Add sorting
            query += " ORDER BY c.date, c.client_id"
            
            print(f"Executing query: {query}\nWith params: {params}")
            cursor.execute(query, params)
            result_columns = [desc[0] for desc in cursor.description]
            rows = cursor.fetchall()
            
            print(f"Found {len(rows)} records")
            if rows:
                print(f"Sample row: {rows[0]}")
            
            df = pd.DataFrame(rows, columns=result_columns)
            
            if df.empty:
                print("No client statistics data found")
                return pd.DataFrame()
            
            return df
            
        except Exception as e:
            error_msg = f"Failed to load client statistics data: {e}\n\n{traceback.format_exc()}"
            print(error_msg)
            QMessageBox.warning(self, "Error", f"Error loading client statistics data: {str(e)}")
            return pd.DataFrame()
        finally:
            if cursor:
                cursor.close()
    
    def load_device_stats(self, conn, start_date, end_date, device_id=None, client_id=None):
        """Load device statistics data
        
        Args:
            conn: Database connection
            start_date: Start date (yyyy-MM-dd)
            end_date: End date (yyyy-MM-dd)
            device_id: Optional device ID, if provided only load data for that device
            client_id: Optional client ID, if provided only load device data for that client
        """
        cursor = None
        try:
            print(f"Loading device stats from {start_date} to {end_date}, device_id: {device_id}, client_id: {client_id}")
            
            # Start new transaction
            conn.rollback()
            cursor = conn.cursor()
            
            # First check if table has data
            cursor.execute("SELECT COUNT(*) FROM device_daily_stats")
            count = cursor.fetchone()[0]
            print(f"Total records in device_daily_stats: {count}")
            
            if count == 0:
                print("No data found in device_daily_stats table")
                return pd.DataFrame()
            
            # Get actual columns in table
            cursor.execute("""
                SELECT column_name 
                FROM information_schema.columns 
                WHERE table_name = 'device_daily_stats'
                ORDER BY ordinal_position
            """)
            columns = [row[0] for row in cursor.fetchall()]
            print(f"Available columns in device_daily_stats: {columns}")
            
            # Build query using only existing columns
            select_columns = ["d.date", "d.client_id", "d.device_index", "d.device_name", "g.client_name"]
            
            # Add metric columns (if they exist)
            metrics = {
                'avg_utilization': 'avg_utilization',
                'avg_temperature': 'avg_temperature',
                'avg_power_usage': 'avg_power_usage',
                'avg_memory_usage': 'avg_memory_usage'
            }
            
            for metric_col, alias in metrics.items():
                if metric_col in columns:
                    select_columns.append(f"d.{metric_col}")
            
            # Build query using LEFT JOIN to get client_name
            query = f"""
                SELECT {', '.join(select_columns)}
                FROM device_daily_stats d
                LEFT JOIN gpu_assets g ON d.client_id = g.client_id
                WHERE d.date >= %s AND d.date <= %s
            """
            
            params = [start_date, end_date]
            
            # Add client filter condition
            if client_id and client_id != "all":
                query += " AND d.client_id = %s"
                params.append(client_id)
                print(f"Filtering for client_id: {client_id}")
                
            # Add device filter condition
            if device_id and device_id != "all":
                query += " AND d.device_index = %s"
                params.append(device_id)
                print(f"Filtering for device_index: {device_id}")
            
            print(f"Executing query: {query}\nWith params: {params}")
            cursor.execute(query, params)
            result_columns = [desc[0] for desc in cursor.description]
            rows = cursor.fetchall()
            
            print(f"Found {len(rows)} records")
            if rows:
                print(f"Sample row: {rows[0]}")
            
            df = pd.DataFrame(rows, columns=result_columns)
            
            if df.empty:
                print("No device statistics data found")
                return pd.DataFrame()
                
            return df
            
        except Exception as e:
            error_msg = f"Failed to load device statistics data: {e}\n\n{traceback.format_exc()}"
            print(error_msg)
            QMessageBox.warning(self, "Error", f"Error loading device statistics data: {str(e)}")
            return pd.DataFrame()
        finally:
            if cursor:
                cursor.close()
                
    def plot_device_stats(self, df):
        """Plot device statistics charts
        
        Args:
            df: DataFrame containing device statistics data
        """
        try:
            if df.empty:
                print("No device data to plot")
                return
                
            # Clear existing charts
            self.device_figure.clear()
            
            # Set Chinese font
            plt.rcParams['font.sans-serif'] = ['Arial Unicode MS']
            plt.rcParams['axes.unicode_minus'] = False
            
            # Create subplots
            ax1 = self.device_figure.add_subplot(311)
            ax2 = self.device_figure.add_subplot(312)
            ax3 = self.device_figure.add_subplot(313)
            
            # convert date to datetime
            df['date'] = pd.to_datetime(df['date'])
            
            # Group by date and device
            if 'device_name' in df.columns:
                group_cols = ['date', 'device_name']
            else:
                group_cols = ['date']
                
            grouped = df.groupby(group_cols).mean().reset_index()
            
            # Plot utilization chart
            if 'avg_utilization' in df.columns:
                if 'device_name' in df.columns:
                    for name, group in grouped.groupby('device_name'):
                        ax1.plot(group['date'], group['avg_utilization'], 'o-', label=name)
                    ax1.legend()
                else:
                    ax1.plot(grouped['date'], grouped['avg_utilization'], 'o-')
                ax1.set_ylabel('GPU utilization (%)')
                ax1.set_title('GPU avg utilization')
                ax1.grid(True)
            
            # Plot temperature chart
            if 'avg_temperature' in df.columns:
                if 'device_name' in df.columns:
                    for name, group in grouped.groupby('device_name'):
                        ax2.plot(group['date'], group['avg_temperature'], 'o-', label=name)
                else:
                    ax2.plot(grouped['date'], grouped['avg_temperature'], 'o-')
                ax2.set_ylabel('temperature (°C)')
                ax2.set_title('GPU avg temperature')
                ax2.grid(True)
            
            # Plot memory usage chart
            if 'avg_memory_usage' in df.columns:
                if 'device_name' in df.columns:
                    for name, group in grouped.groupby('device_name'):
                        ax3.plot(group['date'], group['avg_memory_usage'], 'o-', label=name)
                else:
                    ax3.plot(grouped['date'], grouped['avg_memory_usage'], 'o-')
                ax3.set_ylabel('memory usage (MB)')
                ax3.set_title('GPU avg memory usage')
                ax3.grid(True)
            
            # adjust layout
            self.device_figure.tight_layout()
            
            # Redraw canvas
            self.device_canvas.draw()
            
        except Exception as e:
            error_msg = f"Error plotting device statistics charts: {e}\n\n{traceback.format_exc()}"
            print(error_msg)
            QMessageBox.warning(self, "Error", f"Error plotting device statistics charts: {str(e)}")
    
    def load_table_data(self, conn, start_date, end_date, data_type='client', filter_id=None):
        """load table data

        Args:
            conn: database connection
            start_date: start date (yyyy-MM-dd)
            end_date: end date (yyyy-MM-dd)
            data_type: data type, 'client' or 'device'
            filter_id: optional filter ID, client ID or device index
        """
        cursor = conn.cursor()
        
        try:
            if data_type == 'client':
                query = """
                    SELECT 
                        c.date,
                        c.client_id,
                        g.client_name,
                        c.total_heartbeats,
                        c.avg_cpu_usage,
                        c.avg_memory_usage,
                        c.avg_disk_usage,
                        c.total_network_in_bytes,
                        c.total_network_out_bytes
                    FROM client_daily_stats c
                    LEFT JOIN gpu_assets g ON c.client_id = g.client_id
                    WHERE c.date >= %s AND c.date <= %s
                """
                params = [start_date, end_date]
                
                # add client filter condition
                if filter_id is None:
                    filter_id = self.client_combo.currentData()
                    
                if filter_id and filter_id != "all":
                    query += " AND c.client_id = %s"
                    params.append(filter_id)
                
                # sort
                query += " ORDER BY c.date DESC, c.client_id"
                
            else:  # device stat
                query = """
                    SELECT 
                        d.date,
                        d.client_id,
                        g.client_name,
                        d.device_index,
                        d.device_name,
                        d.avg_utilization,
                        d.avg_temperature,
                        d.avg_power_usage,
                        d.avg_memory_usage
                    FROM device_daily_stats d
                    LEFT JOIN gpu_assets g ON d.client_id = g.client_id
                    WHERE d.date >= %s AND d.date <= %s
                """
                params = [start_date, end_date]
                
                # add client filter condition
                client_id = self.client_combo.currentData()
                if client_id and client_id != "all":
                    query += " AND d.client_id = %s"
                    params.append(client_id)
                
                # add device filter condition
                if filter_id is None:
                    filter_id = self.device_combo.currentData()
                    
                if filter_id and filter_id != "all":
                    query += " AND d.device_index = %s"
                    params.append(filter_id)
                
                # sort
                query += " ORDER BY d.date DESC, d.client_id, d.device_index"
            
            cursor.execute(query, params)
            rows = cursor.fetchall()
            columns = [desc[0] for desc in cursor.description]
            
            # update table
            self.update_table(rows, columns)
            
        except Exception as e:
            error_msg = f"Failed to load table data: {e}\n\n{traceback.format_exc()}"
            print(error_msg)
            QMessageBox.critical(self, "Error", error_msg)
    
    def update_table(self, rows, columns):
        """Update table data"""
        self.data_table.clear()
        self.data_table.setColumnCount(len(columns))
        self.data_table.setRowCount(len(rows))
        
        # set table header
        self.data_table.setHorizontalHeaderLabels(columns)
        
        # fill data
        for row_idx, row in enumerate(rows):
            for col_idx, value in enumerate(row):
                if value is None:
                    item = QTableWidgetItem("")
                else:
                    item = QTableWidgetItem(str(value))
                
                # set cell to be non-editable
                item.setFlags(item.flags() ^ Qt.ItemIsEditable)
                self.data_table.setItem(row_idx, col_idx, item)
        
        # adjust column width
        self.data_table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeToContents)
    
    def plot_client_stats(self, df):
        """Plot client statistics charts"""
        try:
            # clear previous charts
            self.client_figure.clear()
            
            if df.empty or df is None:
                print("No available client statistics data")
                # add placeholder text
                ax = self.client_figure.add_subplot(111)
                ax.text(0.5, 0.5, 'No available client statistics data',
                       horizontalalignment='center',
                       verticalalignment='center',
                       transform=ax.transAxes,
                       fontsize=12)
                ax.axis('off')
                self.client_canvas.draw()
                return
        
            # convert date format
            df['date'] = pd.to_datetime(df['date'])
            
            # create client label column
            if 'client_name' in df.columns and 'client_id' in df.columns:
                df['client_label'] = df.apply(
                    lambda x: f"{x['client_name']} ({x['client_id'].hex()[:8]}...)" 
                             if pd.notna(x['client_name']) and x['client_name'] 
                             else f"Client {x['client_id'].hex()[:8]}...",
                    axis=1
                )
            elif 'client_id' in df.columns:
                df['client_label'] = df['client_id'].apply(lambda x: f"Client {x.hex()[:8]}...")
            else:
                return  # no available client identifier
        
            # create 2x1 subplot layout (vertical arrangement)
            self.client_figure.clear()  # clear previous charts
            
            # create first subplot: CPU and memory usage
            ax1 = self.client_figure.add_subplot(211)
            
            # Plot CPU and memory usage in the same chart
            has_cpu_data = 'avg_cpu_usage' in df.columns
            has_memory_data = 'avg_memory_usage' in df.columns
        
            if has_cpu_data or has_memory_data:
                # Create dual-axis chart
                ax1_twin = ax1.twinx()
                
                # Pre-calculate pivot data
                pivot_cpu = None
                pivot_mem = None
            
                # Define color schemes for CPU and memory
                cpu_colors = ['#1f77b4', '#3498db', '#5dade2', '#85c1e9']  # Blue series
                mem_colors = ['#e74c3c', '#ec7063', '#f1948a', '#f5b7b1']  # Red series
                
                # Plot CPU usage (left axis)
                if has_cpu_data:
                    pivot_cpu = df.pivot(index='date', columns='client_label', values='avg_cpu_usage')
                    lines_cpu = []
                    for i, column in enumerate(pivot_cpu.columns):
                        color_idx = i % len(cpu_colors)
                        line, = ax1.plot(pivot_cpu.index, pivot_cpu[column], 
                                       color=cpu_colors[color_idx], 
                                       marker='o', linewidth=2, markersize=6)
                        lines_cpu.append(line)
                        line.set_label(f'{column} (CPU)')
                    ax1.set_ylabel('CPU Usage (%)', color=cpu_colors[0], fontweight='bold')
                    ax1.tick_params(axis='y', labelcolor=cpu_colors[0])
                
                # Plot memory usage (right axis)
                if has_memory_data:
                    pivot_mem = df.pivot(index='date', columns='client_label', values='avg_memory_usage')
                    lines_mem = []
                    for i, column in enumerate(pivot_mem.columns):
                        color_idx = i % len(mem_colors)
                        line, = ax1_twin.plot(pivot_mem.index, pivot_mem[column], 
                                            color=mem_colors[color_idx],
                                            marker='s', linewidth=2, markersize=5, 
                                            alpha=0.9)
                        lines_mem.append(line)
                        line.set_label(f'{column} (Memory)')
                    ax1_twin.set_ylabel('Memory Usage (%)', color=mem_colors[0], fontweight='bold')
                    ax1_twin.tick_params(axis='y', labelcolor=mem_colors[0])
                
                ax1.set_title('CPU and Memory Average Usage (%)')
                ax1.set_xlabel('')
                ax1.grid(True)
                
                # Merge legends
                lines1, labels1 = ax1.get_legend_handles_labels()
                lines2, labels2 = ax1_twin.get_legend_handles_labels()
                ax1.legend(lines1 + lines2, labels1 + labels2, title='Client', loc='upper right')
            else:
                ax1.text(0.5, 0.5, 'No CPU or memory usage data available',
                        horizontalalignment='center', verticalalignment='center',
                        transform=ax1.transAxes)
        
            # Create second subplot: Network traffic statistics
            ax2 = self.client_figure.add_subplot(212)
            
            # Plot network traffic statistics
            has_network_in = 'total_network_in_bytes' in df.columns
            has_network_out = 'total_network_out_bytes' in df.columns
            
            if has_network_in or has_network_out:
                # create twin axis for network traffic
                ax2_twin = ax2.twinx()
                
                # percolate pivot data
                pivot_network_in = None
                pivot_network_out = None
                
                # set color
                in_colors = ['#2ecc71', '#58d68d', '#82e0aa', '#abebc6']  # Green series
                out_colors = ['#e67e22', '#eb984e', '#f0b27a', '#f5cba7']  # Orange series
                
                # plot network in
                if has_network_in:
                    df_plot = df.copy()
                    df_plot['total_network_in_mb'] = df['total_network_in_bytes'] / (1024 * 1024)
                    pivot_network_in = df_plot.pivot(index='date', columns='client_label', values='total_network_in_mb')
                    lines_in = []
                    for i, column in enumerate(pivot_network_in.columns):
                        color_idx = i % len(in_colors)
                        line, = ax2.plot(pivot_network_in.index, pivot_network_in[column], 
                                       color=in_colors[color_idx],
                                       marker='o', linewidth=2, markersize=6)
                        lines_in.append(line)
                        line.set_label(f'{column} (in)')
                    ax2.set_ylabel('Network In (MB)', color=in_colors[0], fontweight='bold')
                    ax2.tick_params(axis='y', labelcolor=in_colors[0])
                
                # plot network out
                if has_network_out:
                    df_plot = df.copy()
                    df_plot['total_network_out_mb'] = df['total_network_out_bytes'] / (1024 * 1024)
                    pivot_network_out = df_plot.pivot(index='date', columns='client_label', values='total_network_out_mb')
                    lines_out = []
                    for i, column in enumerate(pivot_network_out.columns):
                        color_idx = i % len(out_colors)
                        line, = ax2_twin.plot(pivot_network_out.index, pivot_network_out[column], 
                                            color=out_colors[color_idx],
                                            marker='s', linewidth=2, markersize=5, 
                                            alpha=0.9)
                        lines_out.append(line)
                        line.set_label(f'{column} (out)')
                    ax2_twin.set_ylabel('Network Out (MB)', color=out_colors[0], fontweight='bold')
                    ax2_twin.tick_params(axis='y', labelcolor=out_colors[0])
                
                ax2.set_title('Network Traffic (MB)')
                ax2.set_xlabel('Date')
                ax2.grid(True)
                
                # merge legend
                lines1, labels1 = ax2.get_legend_handles_labels()
                lines2, labels2 = ax2_twin.get_legend_handles_labels()
                ax2.legend(lines1 + lines2, labels1 + labels2, title='Client', loc='upper right')
            else:
                ax2.text(0.5, 0.5, 'No available network traffic data',
                        horizontalalignment='center', verticalalignment='center',
                        transform=ax2.transAxes)
            
            # adjust layout
            self.client_figure.tight_layout()
            
            # force redraw
        
        except Exception as e:
            print(f"Error plotting client stats: {e}")
            traceback.print_exc()
            # show error in chart area
            try:
                self.client_figure.clear()
                ax = self.client_figure.add_subplot(111)
                ax.text(0.5, 0.5, f'Error plotting client stats: {str(e)}',
                       horizontalalignment='center',
                       verticalalignment='center',
                       transform=ax.transAxes,
                       color='red')
                ax.axis('off')
                self.client_canvas.draw()
            except:
                pass
    
    def plot_device_stats(self, df):
        """Plot device stats"""
        self.device_figure.clear()
        
        if df.empty:
            # show error in chart area
            ax = self.device_figure.add_subplot(111)
            ax.text(0.5, 0.5, 'No available device statistics data',
                   horizontalalignment='center',
                   verticalalignment='center',
                   transform=ax.transAxes,
                   fontsize=12)
            ax.axis('off')
            self.device_canvas.draw()
            return
        
        # convert date format
        df['date'] = pd.to_datetime(df['date'])
        
        # create device label column
        if 'device_name' in df.columns and 'device_index' in df.columns:
            df['device_label'] = df.apply(
                lambda x: f"{x['device_name']} (device {x['device_index']})" 
                         if pd.notna(x['device_name']) and x['device_name'] else f"Device {x['device_index']}", 
                axis=1
            )
        elif 'device_index' in df.columns:
            df['device_label'] = df['device_index'].apply(lambda x: f"device {x}")
        else:
            return  # no available device identifier
        
        # create subplots
        ax1 = self.device_figure.add_subplot(211)
        ax2 = self.device_figure.add_subplot(212)
        
        # plot GPU utilization
        if 'avg_utilization' in df.columns:
            pivot_util = df.pivot(index='date', columns='device_label', values='avg_utilization')
            pivot_util.plot(ax=ax1, marker='o')
            ax1.set_title('GPU Average Utilization (%)')
            ax1.set_ylabel('Utilization (%)')
            ax1.grid(True)
            ax1.legend(title='Device')
        else:
            ax1.text(0.5, 0.5, 'No available utilization data', 
                    horizontalalignment='center', verticalalignment='center',
                    transform=ax1.transAxes)
        
        # plot GPU temperature
        if 'avg_temperature' in df.columns:
            pivot_temp = df.pivot(index='date', columns='device_label', values='avg_temperature')
            pivot_temp.plot(ax=ax2, marker='o')
            ax2.set_title('GPU Average Temperature (°C)')
            ax2.set_ylabel('Temperature (°C)')
            ax2.grid(True)
            ax2.legend(title='Device')
        else:
            ax2.text(0.5, 0.5, 'No available temperature data', 
                    horizontalalignment='center', verticalalignment='center',
                    transform=ax2.transAxes)
        
        # adjust layout
        self.device_figure.tight_layout()
        self.device_canvas.draw()
    
    def export_data(self):
        """Export data to CSV file"""
        try:
            # get current tab data
            if self.tabs.currentIndex() == 0:  # client stats
                df = self.get_client_data()
                default_filename = f"client_stats_{datetime.now().strftime('%Y%m%d')}.csv"
            else:  # device stats
                df = self.get_device_data()
                default_filename = f"device_stats_{datetime.now().strftime('%Y%m%d')}.csv"
            
            # save to file
            if df is not None and not df.empty:
                filename, _ = QFileDialog.getSaveFileName(
                    self, "Save File", default_filename, "CSV Files (*.csv)")
                
                if filename:
                    df.to_csv(filename, index=False, encoding='utf-8')
                    QMessageBox.information(self, "Export Success", f"Data exported successfully to:\n{filename}")
            else:
                QMessageBox.warning(self, "Data Empty", "No data to export")
            
        except Exception as e:
            error_msg = f"Export failed: {str(e)}\n\n{traceback.format_exc()}"
            print(error_msg)
            QMessageBox.critical(self, "Export Failed", error_msg)
    
    def get_client_data(self):
        """Get client stats data"""
        conn = None
        try:
            conn = self.get_db_connection()
            if not conn:
                return None
            
            start_date = self.start_date.date().toString("yyyy-MM-dd")
            end_date = self.end_date.date().addDays(1).toString("yyyy-MM-dd")
            
            cursor = conn.cursor()
            
            query = """
                SELECT 
                    c.date,
                    c.client_id,
                    g.client_name,
                    c.total_heartbeats,
                    c.avg_cpu_usage,
                    c.avg_memory_usage,
                    c.avg_disk_usage,
                    c.total_network_in_bytes,
                    c.total_network_out_bytes
                FROM client_daily_stats c
                LEFT JOIN gpu_assets g ON c.client_id = g.client_id
                WHERE c.date >= %s AND c.date <= %s
            """
            params = [start_date, end_date]
            
            # add client filter condition
            client_id = self.client_combo.currentData()
            if client_id and client_id != "all":
                query += " AND c.client_id = %s"
                params.append(client_id)
            
            cursor.execute(query, params)
            
            # get column names
            columns = [desc[0] for desc in cursor.description]
            
            # get data
            rows = cursor.fetchall()
            
            # create DataFrame
            df = pd.DataFrame(rows, columns=columns)
            
            return df
            
        except Exception as e:
            print(f"Failed to get client data: {e}")
            traceback.print_exc()
            return None
        finally:
            if conn:
                self.release_db_connection(conn)
    
    def get_device_data(self):
        """Get device stats data"""
        conn = None
        try:
            conn = self.get_db_connection()
            if not conn:
                return None
                
            start_date = self.start_date.date().toString("yyyy-MM-dd")
            end_date = self.end_date.date().addDays(1).toString("yyyy-MM-dd")
            
            cursor = conn.cursor()
            
            query = """
                SELECT 
                    d.date,
                    d.client_id,
                    g.client_name,
                    d.device_index,
                    d.device_name,
                    d.avg_utilization,
                    d.avg_temperature,
                    d.avg_power_usage,
                    d.avg_memory_usage
                FROM device_daily_stats d
                LEFT JOIN gpu_assets g ON d.client_id = g.client_id
                WHERE d.date >= %s AND d.date <= %s
            """
            params = [start_date, end_date]
            
            # add device filter condition
            device_id = self.device_combo.currentData()
            if device_id and device_id != "all":
                query += " AND d.device_index = %s"
                params.append(device_id)
            
            cursor.execute(query, params)
            
            # get column names
            columns = [desc[0] for desc in cursor.description]
            
            # get data
            rows = cursor.fetchall()
            
            # Create DataFrame
            df = pd.DataFrame(rows, columns=columns)
            
            return df
            
        except Exception as e:
            print(f"Failed to get device data: {e}")
            traceback.print_exc()
            return None
        finally:
            if conn:
                self.release_db_connection(conn)

def main():
    import os
    os.environ['QT_MAC_WANTS_LAYER'] = '1'
    app = QApplication(sys.argv)
    app.setStyle('Fusion')
    
    # Set English locale for date/time widgets
    QLocale.setDefault(QLocale(QLocale.English, QLocale.UnitedStates))
    
    # Handle Ctrl+C gracefully
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    
    # create and show main window
    window = StatsDashboard()
    window.show()
    
    sys.exit(app.exec_())

if __name__ == '__main__':
    main()
